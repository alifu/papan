//! Proto files that belong to a request (plan 08 §3): the scratch protos folder and the
//! copy into `<collection>/protos/` when a scratch request moves to a collection.

use std::path::{Path, PathBuf};

use super::fsio::{self, EntryKind};
use super::paths;
use crate::error::AppError;

pub const PROTOS_DIR: &str = "protos";
pub const SCRATCH_PROTOS_DIR: &str = "scratch-protos";
const PROTO_SUFFIX: &str = ".proto";
const MAX_COPIED_FILES: usize = 200;
const MAX_DEPTH: usize = 16;

struct ProtoFile {
    rel: String,
    bytes: Vec<u8>,
}

/// Copies every `.proto` under `from` to `to` under the same relative names. Identical
/// files are skipped; a file with other content refuses the whole copy before anything is
/// written. Symlinks are never followed. Returns the number of files written.
pub fn copy_protos(from: &Path, to: &Path) -> Result<usize, AppError> {
    let mut files = Vec::new();
    collect(from, "", 0, &mut files)?;
    let pending = pending_files(to, files)?;
    if pending.is_empty() {
        return Ok(0);
    }
    fsio::create_dir_all(to)?;
    for file in &pending {
        let target = paths::resolve(to, &file.rel)?;
        if let Some(parent) = target.parent() {
            fsio::create_dir_all(parent)?;
        }
        fsio::write_atomic(&target, &file.bytes)?;
    }
    Ok(pending.len())
}

fn collect(dir: &Path, rel: &str, depth: usize, out: &mut Vec<ProtoFile>) -> Result<(), AppError> {
    if fsio::entry_kind(dir) != Some(EntryKind::Dir) {
        return Ok(());
    }
    if depth > MAX_DEPTH {
        return Err(AppError::invalid_input(
            "The protos folder is nested too deeply.",
        ));
    }
    for entry in fsio::list_dir(dir)? {
        let child_rel = paths::join_rel(rel, &entry.name);
        let child = dir.join(&entry.name);
        match entry.kind {
            EntryKind::Dir => collect(&child, &child_rel, depth + 1, out)?,
            EntryKind::File if entry.name.ends_with(PROTO_SUFFIX) => {
                paths::segments(&child_rel)?;
                if out.len() >= MAX_COPIED_FILES {
                    return Err(AppError::invalid_input(
                        "Too many proto files to copy to the collection.",
                    ));
                }
                out.push(ProtoFile {
                    rel: child_rel,
                    bytes: fsio::read_bounded(&child)?,
                });
            }
            _ => {}
        }
    }
    Ok(())
}

/// The files that still need writing; errors on the first one with other content.
fn pending_files(to: &Path, files: Vec<ProtoFile>) -> Result<Vec<ProtoFile>, AppError> {
    let target_exists = fsio::entry_kind(to).is_some();
    let mut pending = Vec::new();
    for file in files {
        if !target_exists {
            pending.push(file);
            continue;
        }
        let target = paths::resolve(to, &file.rel)?;
        match fsio::entry_kind(&target) {
            None => pending.push(file),
            Some(EntryKind::File) if fsio::read_bounded(&target)? == file.bytes => {}
            Some(_) => {
                return Err(AppError::invalid_input(format!(
                    "{PROTOS_DIR}/{} already exists with other content.",
                    file.rel
                )))
            }
        }
    }
    Ok(pending)
}

/// The scratch protos folder of `id` under `app_data` (not created).
pub fn scratch_protos_dir(app_data: &Path, id: &str) -> PathBuf {
    app_data.join(SCRATCH_PROTOS_DIR).join(id)
}

#[cfg(test)]
#[path = "grpc_protos_tests.rs"]
mod tests;
