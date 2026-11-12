//! `collections.json` in the app data dir: which collection folders are open (plan §6).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::fsio;
use crate::error::AppError;

const REGISTRY_FILE: &str = "collections.json";
const REGISTRY_VERSION: u32 = 1;
/// Closed collections whose ids are remembered, so local values and secrets survive a
/// close and reopen (plan 04 §3.3).
const MAX_CLOSED: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub id: String,
    pub path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct RegistryFile {
    version: u32,
    collections: Vec<RegistryEntry>,
    #[serde(default)]
    closed: Vec<RegistryEntry>,
}

/// Entries must carry a UUID id and an absolute path; anything else is dropped on load.
fn is_valid(entry: &RegistryEntry) -> bool {
    let valid = uuid::Uuid::parse_str(&entry.id).is_ok() && entry.path.is_absolute();
    if !valid {
        tracing::warn!(id = %entry.id, path = %entry.path.display(), "dropping an invalid collections.json entry");
    }
    valid
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registry {
    file: PathBuf,
    entries: Vec<RegistryEntry>,
    /// Newest first.
    closed: Vec<RegistryEntry>,
}

impl Registry {
    /// Loads the registry; a missing or malformed file counts as empty.
    pub fn load(app_data: &Path) -> Self {
        let file = app_data.join(REGISTRY_FILE);
        let (entries, closed) = match fsio::entry_kind(&file) {
            None => (Vec::new(), Vec::new()),
            Some(_) => fsio::read_bounded(&file)
                .and_then(|bytes| {
                    serde_json::from_slice::<RegistryFile>(&bytes).map_err(|source| {
                        AppError::invalid_input("collections.json is malformed.")
                            .with_detail(source.to_string())
                    })
                })
                .map(|parsed| {
                    let valid =
                        |list: Vec<RegistryEntry>| list.into_iter().filter(is_valid).collect();
                    (valid(parsed.collections), valid(parsed.closed))
                })
                .unwrap_or_else(|error| {
                    tracing::warn!(detail = ?error.detail, "{error} Starting with no collections.");
                    (Vec::new(), Vec::new())
                }),
        };
        Self {
            file,
            entries,
            closed,
        }
    }

    pub fn entries(&self) -> &[RegistryEntry] {
        &self.entries
    }

    pub fn find(&self, id: &str) -> Result<&RegistryEntry, AppError> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| AppError::not_found("That collection is not open."))
    }

    /// Adds `path` (already canonical) unless it is open already; returns its entry.
    pub fn add(&mut self, path: &Path) -> Result<RegistryEntry, AppError> {
        if let Some(existing) = self.entries.iter().find(|entry| entry.path == path) {
            return Ok(existing.clone());
        }
        let reused = self.closed.iter().find(|entry| entry.path == path).cloned();
        let entry = reused.unwrap_or_else(|| RegistryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            path: path.to_path_buf(),
        });
        let entries = self
            .entries
            .iter()
            .cloned()
            .chain([entry.clone()])
            .collect();
        let closed = self
            .closed
            .iter()
            .filter(|closed| closed.id != entry.id)
            .cloned()
            .collect();
        self.save(entries, closed)?;
        Ok(entry)
    }

    pub fn remove(&mut self, id: &str) -> Result<(), AppError> {
        let removed = self.find(id)?.clone();
        let entries = self
            .entries
            .iter()
            .filter(|entry| entry.id != id)
            .cloned()
            .collect();
        let closed = std::iter::once(removed.clone())
            .chain(
                self.closed
                    .iter()
                    .filter(|entry| entry.path != removed.path)
                    .cloned(),
            )
            .take(MAX_CLOSED)
            .collect();
        self.save(entries, closed)
    }

    fn save(
        &mut self,
        entries: Vec<RegistryEntry>,
        closed: Vec<RegistryEntry>,
    ) -> Result<(), AppError> {
        let file = RegistryFile {
            version: REGISTRY_VERSION,
            collections: entries,
            closed,
        };
        let text = serde_json::to_string_pretty(&file).map_err(|source| {
            AppError::internal("Could not save the collection list.")
                .with_detail(source.to_string())
        })?;
        fsio::write_atomic(&self.file, format!("{text}\n").as_bytes())?;
        self.entries = file.collections;
        self.closed = file.closed;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[test]
    fn survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::load(dir.path());

        let entry = registry.add(Path::new("/a")).unwrap();
        let again = registry.add(Path::new("/a")).unwrap();
        registry.add(Path::new("/b")).unwrap();
        let reloaded = Registry::load(dir.path());

        assert_eq!(entry, again);
        assert_eq!(reloaded, registry);
        assert_eq!(reloaded.entries().len(), 2);
        assert_eq!(reloaded.find(&entry.id).unwrap().path, Path::new("/a"));
    }

    #[test]
    fn remove_forgets_and_rejects_unknown_ids() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::load(dir.path());
        let entry = registry.add(Path::new("/a")).unwrap();

        registry.remove(&entry.id).unwrap();

        assert!(Registry::load(dir.path()).entries().is_empty());
        assert_eq!(
            registry.remove(&entry.id).unwrap_err().code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn reopening_a_closed_path_reuses_its_id() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::load(dir.path());
        let entry = registry.add(Path::new("/a")).unwrap();

        registry.remove(&entry.id).unwrap();
        let mut reloaded = Registry::load(dir.path());
        let again = reloaded.add(Path::new("/a")).unwrap();
        let other = reloaded.add(Path::new("/b")).unwrap();

        assert_eq!(again, entry);
        assert_ne!(other.id, entry.id);
        assert!(reloaded.closed.is_empty());
    }

    #[test]
    fn closed_list_is_capped_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::load(dir.path());
        for n in 0..=MAX_CLOSED {
            let entry = registry.add(Path::new(&format!("/c{n}"))).unwrap();
            registry.remove(&entry.id).unwrap();
        }

        assert_eq!(registry.closed.len(), MAX_CLOSED);
        assert_eq!(
            registry.closed[0].path,
            Path::new(&format!("/c{MAX_CLOSED}"))
        );
    }

    #[test]
    fn invalid_entries_are_dropped_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let good = uuid::Uuid::new_v4();
        let text = format!(
            r#"{{"version":1,"collections":[
                {{"id":"{good}","path":"/ok"}},
                {{"id":"not-a-uuid","path":"/x"}},
                {{"id":"{}","path":"relative/path"}}
            ]}}"#,
            uuid::Uuid::new_v4()
        );
        fsio::write_atomic(&dir.path().join(REGISTRY_FILE), text.as_bytes()).unwrap();

        let registry = Registry::load(dir.path());

        assert_eq!(registry.entries().len(), 1);
        assert_eq!(registry.entries()[0].id, good.to_string());
    }

    #[test]
    fn malformed_file_is_treated_as_empty_and_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(REGISTRY_FILE);
        fsio::write_atomic(&file, b"{not json").unwrap();

        let registry = Registry::load(dir.path());

        assert!(registry.entries().is_empty());
        assert_eq!(fsio::read_bounded(&file).unwrap(), b"{not json");
    }
}
