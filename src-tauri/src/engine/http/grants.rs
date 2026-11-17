//! Files the user picked in the native dialog and may upload (plan 03 §5). A path that
//! only appears in a request file is never sent: the webview cannot add a grant. Each
//! grant belongs to the collection (or scratch request) it was picked for, so a path
//! planted in another collection is refused even when the same file was picked elsewhere.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};

use super::types::PickedFile;
use crate::error::AppError;
use crate::store::fsio;
use crate::store::types::RequestLocation;

pub const MAX_GRANTS: usize = 500;
const GRANTS_FILE: &str = "file_grants.json";

/// The scope a location's grants live in.
pub fn grant_scope(location: &RequestLocation) -> String {
    match location {
        RequestLocation::Scratch { id } => format!("scratch:{id}"),
        RequestLocation::Collection { collection_id, .. } => format!("collection:{collection_id}"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Grant {
    scope: String,
    /// Canonical.
    path: PathBuf,
}

#[derive(Debug)]
pub struct Grants {
    file: PathBuf,
    /// Newest first.
    grants: Mutex<Vec<Grant>>,
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn refused(path: &Path) -> AppError {
    AppError::invalid_input(format!(
        "Choose the file again to allow uploading it: {}",
        file_name(path)
    ))
}

/// The canonical path (symlinks resolved) of an existing regular file.
fn canonical_file(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path)
        .ok()
        .filter(|canonical| canonical.is_file())
}

impl Grants {
    /// Loads `<app_data>/file_grants.json`; a missing or malformed file counts as empty.
    pub fn load(app_data: &Path) -> Self {
        let file = app_data.join(GRANTS_FILE);
        let grants = std::fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<Grant>>(&bytes).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|grant| grant.path.is_absolute())
            .take(MAX_GRANTS)
            .collect();
        Self {
            file,
            grants: Mutex::new(grants),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Grant>> {
        self.grants.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Puts `added` first, drops their older duplicates, caps and saves.
    fn update(&self, added: Vec<Grant>, removed: impl Fn(&Grant) -> bool) -> Result<(), AppError> {
        let updated: Vec<Grant> = {
            let mut grants = self.lock();
            let next: Vec<Grant> = added
                .iter()
                .cloned()
                .chain(
                    grants
                        .iter()
                        .filter(|grant| !added.contains(grant) && !removed(grant))
                        .cloned(),
                )
                .take(MAX_GRANTS)
                .collect();
            grants.clone_from(&next);
            next
        };
        let bytes = serde_json::to_vec(&updated).map_err(|source| {
            AppError::internal("Could not save the file grants.").with_detail(source.to_string())
        })?;
        fsio::write_atomic(&self.file, &bytes).map(|_| ())
    }

    /// Grants a file picked in the native dialog to `scope` and describes it.
    pub fn grant(&self, scope: &str, picked: &Path) -> Result<PickedFile, AppError> {
        let canonical = canonical_file(picked)
            .ok_or_else(|| AppError::invalid_input("That file cannot be uploaded."))?;
        let size = std::fs::metadata(&canonical)?.len();
        let grant = Grant {
            scope: scope.to_string(),
            path: canonical.clone(),
        };
        self.update(vec![grant], |_| false)?;
        Ok(PickedFile {
            path: canonical.display().to_string(),
            name: file_name(&canonical),
            size_bytes: u32::try_from(size).unwrap_or(u32::MAX),
        })
    }

    /// The canonical path of `path` when it is an existing regular file granted to `scope`.
    pub fn check(&self, scope: &str, path: &Path) -> Result<PathBuf, AppError> {
        let canonical = canonical_file(path).ok_or_else(|| refused(path))?;
        let granted = self
            .lock()
            .iter()
            .any(|grant| grant.scope == scope && grant.path == canonical);
        if granted {
            Ok(canonical)
        } else {
            Err(refused(path))
        }
    }

    /// Moves every grant of `from` to `to` (a scratch request moved into a collection).
    pub fn carry(&self, from: &str, to: &str) -> Result<(), AppError> {
        let moved: Vec<Grant> = self
            .lock()
            .iter()
            .filter(|grant| grant.scope == from)
            .map(|grant| Grant {
                scope: to.to_string(),
                path: grant.path.clone(),
            })
            .collect();
        if moved.is_empty() {
            return Ok(());
        }
        self.update(moved, |grant| grant.scope == from)
    }
}

#[cfg(test)]
#[path = "grants_tests.rs"]
mod tests;
