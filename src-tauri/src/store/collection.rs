//! One collection folder on disk: open, create, read and write requests, and the helpers
//! the tree modules share (plan §3). Scanning lives in `scan`, tree operations in
//! `tree_ops`, change detection in `fingerprint`. Every webview-supplied path goes
//! through `paths::resolve`.

use std::path::{Path, PathBuf};

use super::fsio::{self, DirEntry, EntryKind};
use super::model::{self, CollectionFile, FolderFile, RequestFile};
use super::paths::{self, FOLDER_FILE, REQUEST_SUFFIX};
use super::request_model;
use super::types::RequestContent;
use crate::error::{AppError, ErrorCode};

pub const COLLECTION_FILE: &str = "papan.collection.yaml";
pub const REQUESTS_DIR: &str = "requests";
pub const MAX_DEPTH: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collection {
    pub root: PathBuf,
    pub name: String,
}

pub(super) fn is_request_name(name: &str) -> bool {
    name.ends_with(REQUEST_SUFFIX) && name.len() > REQUEST_SUFFIX.len()
}

/// Entries shown in the tree: real directories and `*.http.yaml` files with safe names.
pub(super) fn is_listable(entry: &DirEntry) -> bool {
    let safe = paths::is_valid_segment(&entry.name);
    let shown = match entry.kind {
        EntryKind::Dir => true,
        EntryKind::File => is_request_name(&entry.name),
        EntryKind::Symlink | EntryKind::Other => false,
    };
    if !safe || !shown {
        tracing::debug!(name = %entry.name, kind = ?entry.kind, "skipped in collection scan");
    }
    safe && shown
}

pub(super) fn folder_missing() -> AppError {
    AppError::not_found("That folder does not exist.")
}

pub(super) fn item_missing() -> AppError {
    AppError::not_found("That item does not exist.")
}

impl Collection {
    /// Opens the collection at `root` (an absolute path from the picker or the registry).
    pub fn open(root: &Path) -> Result<Self, AppError> {
        let file = root.join(COLLECTION_FILE);
        if fsio::entry_kind(&file) != Some(EntryKind::File) {
            return Err(
                AppError::invalid_input("That folder is not a Papan collection.")
                    .with_detail(root.display().to_string()),
            );
        }
        match fsio::entry_kind(&root.join(REQUESTS_DIR)) {
            None | Some(EntryKind::Dir) => {}
            Some(_) => {
                return Err(AppError::invalid_input(
                    "The collection's requests folder is not valid.",
                )
                .with_detail(root.display().to_string()))
            }
        }
        let parsed = CollectionFile::parse(COLLECTION_FILE, &fsio::read_bounded(&file)?)?;
        Ok(Self {
            root: root.to_path_buf(),
            name: parsed.name,
        })
    }

    /// Creates `<parent>/<slug>` with a collection file and an empty `requests/`.
    /// A half-made folder is removed again if a later step fails.
    pub fn create(parent: &Path, name: &str) -> Result<Self, AppError> {
        let name = model::clean_name(name)?;
        if fsio::entry_kind(parent) != Some(EntryKind::Dir) {
            return Err(folder_missing().with_detail(parent.display().to_string()));
        }
        // No siblings: only the reserved Windows names are avoided (`CON` → `con-2`).
        let root = parent.join(paths::unique_stem(&paths::slugify(&name), &[])?);
        if fsio::entry_kind(&root).is_some() {
            return Err(
                AppError::invalid_input("A folder with that name already exists there.")
                    .with_detail(root.display().to_string()),
            );
        }
        fsio::create_dir(&root)?;
        let filled = CollectionFile::new(name)
            .to_yaml()
            .and_then(|text| fsio::write_atomic(&root.join(COLLECTION_FILE), text.as_bytes()))
            .and_then(|_| fsio::create_dir(&root.join(REQUESTS_DIR)))
            .and_then(|()| Self::open(&root));
        if filled.is_err() {
            if let Err(error) = fsio::remove_dir_all(&root) {
                tracing::warn!(detail = ?error.detail, "could not remove a half-made collection");
            }
        }
        filled
    }

    /// `<collection>/protos`, the root of the gRPC proto files (may not exist yet).
    pub fn protos_root(&self) -> PathBuf {
        self.root.join(super::grpc_protos::PROTOS_DIR)
    }

    pub fn requests_root(&self) -> PathBuf {
        self.root.join(REQUESTS_DIR)
    }

    /// Removes temp files left by an interrupted write anywhere in the collection.
    pub fn sweep_temp_files(&self) -> usize {
        let root = self.requests_root();
        let in_requests = if fsio::entry_kind(&root) == Some(EntryKind::Dir) {
            sweep_tree(&root, 0)
        } else {
            0
        };
        fsio::sweep_temp_files(&self.root) + in_requests
    }

    pub fn read_request(&self, rel: &str) -> Result<RequestFile, AppError> {
        let (_, entry) = paths::split_rel(rel);
        if !is_request_name(entry) {
            return Err(
                AppError::invalid_input("That is not a request.").with_detail(format!("{rel:?}"))
            );
        }
        let abs = paths::resolve(&self.requests_root(), rel)?;
        read_request_file(&abs)
    }

    pub fn write_request(&self, rel: &str, content: &RequestContent) -> Result<(), AppError> {
        request_model::validate_content("Request", content)?;
        let current = self.read_request(rel)?;
        let abs = paths::resolve(&self.requests_root(), rel)?;
        let updated = current.with_content(content)?;
        fsio::write_atomic(&abs, updated.to_yaml()?.as_bytes())?;
        Ok(())
    }

    /// Creates `requests/` when a mutation needs it and it is missing.
    fn ensure_requests_dir(&self) -> Result<(), AppError> {
        let root = self.requests_root();
        match fsio::entry_kind(&root) {
            None => fsio::create_dir(&root),
            Some(EntryKind::Dir) => Ok(()),
            Some(_) => Err(AppError::invalid_input("That path is not allowed.")),
        }
    }

    pub(super) fn dir_at(&self, rel: &str) -> Result<PathBuf, AppError> {
        self.ensure_requests_dir()?;
        let abs = paths::resolve(&self.requests_root(), rel)?;
        if fsio::entry_kind(&abs) == Some(EntryKind::Dir) {
            Ok(abs)
        } else {
            Err(folder_missing().with_detail(format!("{rel:?}")))
        }
    }

    /// A non-root item: its absolute path and kind (a real file or directory).
    pub(super) fn node_at(&self, rel: &str) -> Result<(PathBuf, EntryKind), AppError> {
        if rel.is_empty() {
            return Err(AppError::invalid_input(
                "The collection root cannot be changed.",
            ));
        }
        let abs = paths::resolve(&self.requests_root(), rel)?;
        match fsio::entry_kind(&abs) {
            Some(kind @ (EntryKind::Dir | EntryKind::File)) => Ok((abs, kind)),
            _ => Err(item_missing().with_detail(format!("{rel:?}"))),
        }
    }
}

fn sweep_tree(dir: &Path, depth: usize) -> usize {
    let below: usize = fsio::list_dir(dir)
        .unwrap_or_default()
        .iter()
        .filter(|entry| entry.kind == EntryKind::Dir && is_listable(entry))
        .filter(|_| depth + 1 < MAX_DEPTH)
        .map(|entry| sweep_tree(&dir.join(&entry.name), depth + 1))
        .sum();
    fsio::sweep_temp_files(dir) + below
}

fn label_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub(super) fn read_request_file(abs: &Path) -> Result<RequestFile, AppError> {
    let bytes = fsio::read_bounded(abs).map_err(|error| match error.code {
        ErrorCode::NotFound => item_missing(),
        _ => error,
    })?;
    RequestFile::parse(&label_of(abs), &bytes)
        .map_err(|error| error.with_detail(abs.display().to_string()))
}

/// The folder's `_folder.yaml`, or an empty one when the file does not exist.
pub(super) fn read_folder_file(dir: &Path) -> Result<FolderFile, AppError> {
    let path = dir.join(FOLDER_FILE);
    match fsio::entry_kind(&path) {
        None => Ok(FolderFile::default()),
        Some(EntryKind::File) => FolderFile::parse(FOLDER_FILE, &fsio::read_bounded(&path)?)
            .map_err(|error| error.with_detail(path.display().to_string())),
        Some(_) => Err(AppError::invalid_input("That path is not allowed.")),
    }
}

/// Writes `_folder.yaml`, removing it instead when nothing is left to store.
pub(super) fn write_folder_file(dir: &Path, file: &FolderFile) -> Result<(), AppError> {
    let path = dir.join(FOLDER_FILE);
    if file.is_empty() {
        return match fsio::entry_kind(&path) {
            Some(EntryKind::File) => fsio::remove_file(&path),
            _ => Ok(()),
        };
    }
    fsio::write_atomic(&path, file.to_yaml()?.as_bytes()).map(|_| ())
}

#[cfg(test)]
#[path = "collection_tests.rs"]
mod tests;
