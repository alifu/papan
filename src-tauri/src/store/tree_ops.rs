//! Tree operations on a collection and their effect on `order` (plan §3.4, §4).

use std::path::Path;

use super::collection::{
    is_listable, is_request_name, read_folder_file, read_request_file, write_folder_file,
    Collection, MAX_DEPTH,
};
use super::fsio::{self, EntryKind};
use super::model::{self, FolderFile, RequestFile};
use super::paths::{self, FOLDER_FILE, REQUEST_SUFFIX};
use super::protocol_model;
use super::scan::arrange;
use super::types::RequestKind;
use crate::error::AppError;

enum Content {
    Folder(FolderFile),
    Request(RequestFile),
}

fn too_deep() -> AppError {
    AppError::invalid_input("Folders cannot be nested that deeply.")
}

/// Refuses a destination that already exists. The sibling check compares ASCII case only;
/// a case-folding filesystem (APFS, NTFS) can still map a new name onto an existing one.
fn ensure_free(dest: &Path) -> Result<(), AppError> {
    match fsio::entry_kind(dest) {
        None => Ok(()),
        Some(_) => Err(
            AppError::invalid_input("An item with that name already exists.")
                .with_detail(dest.display().to_string()),
        ),
    }
}

impl Collection {
    pub fn create_folder(&self, parent_rel: &str, name: &str) -> Result<String, AppError> {
        let name = model::clean_name(name)?;
        if paths::segments(parent_rel)?.len() + 1 >= MAX_DEPTH {
            return Err(too_deep());
        }
        let parent = self.dir_at(parent_rel)?;
        let stem = paths::unique_stem(&paths::slugify(&name), &sibling_names(&parent)?)?;
        let dir = parent.join(&stem);
        ensure_free(&dir)?;
        fsio::create_dir(&dir)?;
        write_folder_file(&dir, &FolderFile::default().with_name(&name))?;
        Ok(paths::join_rel(parent_rel, &stem))
    }

    pub fn create_request(
        &self,
        parent_rel: &str,
        name: &str,
        kind: RequestKind,
    ) -> Result<String, AppError> {
        let name = model::clean_name(name)?;
        self.add_request(parent_rel, &protocol_model::new_request_file(name, kind)?)
    }

    /// Writes `file` as a new request in `parent_rel`, named after its display name.
    pub fn add_request(&self, parent_rel: &str, file: &RequestFile) -> Result<String, AppError> {
        file.validate("Request")?;
        let parent = self.dir_at(parent_rel)?;
        let stem = paths::unique_stem(&paths::slugify(&file.name), &sibling_names(&parent)?)?;
        let entry = format!("{stem}{REQUEST_SUFFIX}");
        let dest = parent.join(&entry);
        ensure_free(&dest)?;
        fsio::write_atomic(&dest, file.to_yaml()?.as_bytes())?;
        Ok(paths::join_rel(parent_rel, &entry))
    }

    pub fn rename(&self, rel: &str, name: &str) -> Result<String, AppError> {
        let name = model::clean_name(name)?;
        let (abs, kind) = self.node_at(rel)?;
        let (parent_rel, old) = paths::split_rel(rel);
        let parent = self.dir_at(parent_rel)?;
        let content = match kind {
            EntryKind::Dir => Content::Folder(read_folder_file(&abs)?),
            _ if is_request_name(old) => Content::Request(read_request_file(&abs)?),
            _ => {
                return Err(AppError::invalid_input(
                    "Only requests and folders can be renamed.",
                ))
            }
        };
        let slug = paths::slugify(&name);
        let new_entry = if paths::stem(old).eq_ignore_ascii_case(&slug) {
            old.to_string()
        } else {
            let others: Vec<String> = sibling_names(&parent)?
                .into_iter()
                .filter(|sibling| sibling != old)
                .collect();
            let stem = paths::unique_stem(&slug, &others)?;
            match content {
                Content::Folder(_) => stem,
                Content::Request(_) => format!("{stem}{REQUEST_SUFFIX}"),
            }
        };
        let new_abs = parent.join(&new_entry);
        if new_entry != old {
            ensure_free(&new_abs)?;
            fsio::rename(&abs, &new_abs)?;
        }
        match content {
            Content::Folder(file) => write_folder_file(&new_abs, &file.with_name(&name))?,
            Content::Request(file) => {
                fsio::write_atomic(&new_abs, file.with_name(&name).to_yaml()?.as_bytes())?;
            }
        }
        if new_entry != old {
            replace_in_order(&parent, old, Some(&new_entry))?;
        }
        Ok(paths::join_rel(parent_rel, &new_entry))
    }

    pub fn delete(&self, rel: &str) -> Result<(), AppError> {
        let (abs, kind) = self.node_at(rel)?;
        let (parent_rel, entry) = paths::split_rel(rel);
        match kind {
            EntryKind::Dir => fsio::remove_dir_all(&abs)?,
            _ if is_request_name(entry) || entry == FOLDER_FILE => fsio::remove_file(&abs)?,
            _ => {
                return Err(AppError::invalid_input(
                    "Only requests and folders can be deleted.",
                ))
            }
        }
        if entry != FOLDER_FILE {
            let parent = self.dir_at(parent_rel)?;
            replace_in_order(&parent, entry, None)?;
        }
        Ok(())
    }

    /// Moves `rel` into `target_rel` at `index`: the position among the target's children
    /// after the node has left its old place, clamped. Within one folder this reorders.
    pub fn move_node(&self, rel: &str, target_rel: &str, index: u32) -> Result<String, AppError> {
        let (abs, kind) = self.node_at(rel)?;
        let (parent_rel, entry) = paths::split_rel(rel);
        if kind != EntryKind::Dir && !is_request_name(entry) {
            return Err(AppError::invalid_input(
                "Only requests and folders can be moved.",
            ));
        }
        let target = self.dir_at(target_rel)?;
        let into_itself = target_rel == rel || target_rel.starts_with(&format!("{rel}/"));
        if kind == EntryKind::Dir && into_itself {
            return Err(AppError::invalid_input(
                "A folder cannot be moved into itself or one of its folders.",
            ));
        }
        if parent_rel == target_rel {
            write_order(&target, insert_at(current_order(&target)?, entry, index))?;
            return Ok(rel.to_string());
        }
        let target_depth = paths::segments(target_rel)?.len();
        if kind == EntryKind::Dir && target_depth + folder_height(&abs, 0) >= MAX_DEPTH {
            return Err(too_deep());
        }
        let siblings = sibling_names(&target)?;
        let free = paths::unique_stem(paths::stem(entry), &siblings)?;
        let new_entry = match (free == paths::stem(entry), kind) {
            (true, _) => entry.to_string(),
            (false, EntryKind::Dir) => free,
            (false, _) => format!("{free}{REQUEST_SUFFIX}"),
        };
        let dest = target.join(&new_entry);
        ensure_free(&dest)?;
        fsio::rename(&abs, &dest)?;
        replace_in_order(&self.dir_at(parent_rel)?, entry, None)?;
        write_order(
            &target,
            insert_at(current_order(&target)?, &new_entry, index),
        )?;
        Ok(paths::join_rel(target_rel, &new_entry))
    }
}

/// Levels of folders from `dir` down (1 for a folder without subfolders), capped.
fn folder_height(dir: &Path, depth: usize) -> usize {
    if depth >= MAX_DEPTH {
        return 1;
    }
    let deepest_child = fsio::list_dir(dir)
        .unwrap_or_default()
        .iter()
        .filter(|entry| entry.kind == EntryKind::Dir && is_listable(entry))
        .map(|entry| folder_height(&dir.join(&entry.name), depth + 1))
        .max()
        .unwrap_or(0);
    1 + deepest_child
}

fn insert_at(names: Vec<String>, name: &str, index: u32) -> Vec<String> {
    let without: Vec<String> = names.into_iter().filter(|other| other != name).collect();
    let at = usize::try_from(index)
        .unwrap_or(usize::MAX)
        .min(without.len());
    let (head, tail) = without.split_at(at);
    head.iter()
        .cloned()
        .chain(std::iter::once(name.to_string()))
        .chain(tail.iter().cloned())
        .collect()
}

fn sibling_names(dir: &Path) -> Result<Vec<String>, AppError> {
    Ok(fsio::list_dir(dir)?
        .into_iter()
        .map(|entry| entry.name)
        .collect())
}

fn listed_names(dir: &Path) -> Result<Vec<String>, AppError> {
    Ok(fsio::list_dir(dir)?
        .into_iter()
        .filter(is_listable)
        .map(|entry| entry.name)
        .collect())
}

fn current_order(dir: &Path) -> Result<Vec<String>, AppError> {
    let order = read_folder_file(dir)?.order;
    Ok(arrange(&listed_names(dir)?, &order))
}

fn write_order(dir: &Path, names: Vec<String>) -> Result<(), AppError> {
    let file = read_folder_file(dir)?;
    write_folder_file(dir, &file.with_order(names))
}

/// Replaces (or with `None` removes) `old` in the folder's `order`, only if it was listed.
fn replace_in_order(dir: &Path, old: &str, new: Option<&str>) -> Result<(), AppError> {
    let Ok(file) = read_folder_file(dir) else {
        return Ok(());
    };
    if !file.order.iter().any(|name| name == old) {
        return Ok(());
    }
    let order = file
        .order
        .iter()
        .filter_map(|name| {
            if name == old {
                new.map(str::to_string)
            } else {
                Some(name.clone())
            }
        })
        .collect();
    write_folder_file(dir, &file.with_order(order))
}
