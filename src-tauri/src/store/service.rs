//! `Store`: the plain-Rust API behind the Tauri commands. It ties the registry, the open
//! collections and scratch together and keeps each collection's fingerprint (plan §5).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::collection::Collection;
use super::fsio;
use super::history::{History, HistoryRecord, Sent};
use super::model::RequestFile;
use super::registry::{Registry, RegistryEntry};
use super::request_emit::normalise;
use super::scratch::Scratch;
use super::secrets::{redact, SessionSecrets};
use super::types::{
    CollectionSummary, CollectionTree, RequestContent, RequestDoc, RequestLocation, ScratchItem,
    TreeChange,
};
use crate::engine::http::types::HistoryEntry;
use crate::error::AppError;

#[derive(Debug)]
pub struct Store {
    registry: Registry,
    scratch: Scratch,
    fingerprints: HashMap<String, u64>,
    secrets: SessionSecrets,
    history: History,
}

fn summary(entry: &RegistryEntry, collection: Option<&Collection>) -> CollectionSummary {
    let fallback = || {
        entry
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    CollectionSummary {
        id: entry.id.clone(),
        name: collection.map_or_else(fallback, |c| c.name.clone()),
        path: entry.path.display().to_string(),
        available: collection.is_some(),
    }
}

fn tree_of(entry: &RegistryEntry, collection: Option<&Collection>) -> CollectionTree {
    CollectionTree {
        collection: summary(entry, collection),
        children: collection.map(Collection::tree).unwrap_or_default(),
    }
}

/// Opens a registered collection only while its folder still canonicalises to the stored
/// (canonical) path, so a folder swapped for a symlink becomes unavailable.
fn open_registered(entry: &RegistryEntry) -> Result<Collection, AppError> {
    let canonical = fsio::canonicalize(&entry.path)?;
    if canonical != entry.path {
        return Err(
            AppError::invalid_input("The collection folder has moved or is now a link.")
                .with_detail(entry.path.display().to_string()),
        );
    }
    Collection::open(&canonical)
}

fn doc(file: RequestFile) -> Result<RequestDoc, AppError> {
    Ok(RequestDoc {
        content: file.content()?,
        name: file.name,
    })
}

impl Store {
    /// A store over `app_data` (created if missing). Tests pass a temp dir.
    pub fn new(app_data: &Path) -> Result<Self, AppError> {
        fsio::create_dir_all(app_data)?;
        Ok(Self {
            registry: Registry::load(app_data),
            scratch: Scratch::new(app_data)?,
            fingerprints: HashMap::new(),
            secrets: SessionSecrets::default(),
            history: History::new(app_data),
        })
    }

    fn entry(&self, id: &str) -> Result<RegistryEntry, AppError> {
        self.registry.find(id).cloned()
    }

    fn open_entry(&self, id: &str) -> Result<(RegistryEntry, Collection), AppError> {
        let entry = self.entry(id)?;
        let collection = open_registered(&entry)?;
        Ok((entry, collection))
    }

    fn remember(&mut self, id: &str, collection: &Collection) {
        self.fingerprints
            .insert(id.to_string(), collection.fingerprint());
    }

    /// Every open collection; a folder that cannot be opened is `available: false`.
    pub fn list_collections(&mut self) -> Vec<CollectionTree> {
        let entries = self.registry.entries().to_vec();
        entries
            .iter()
            .map(|entry| {
                let collection = open_registered(entry)
                    .map_err(|error| tracing::warn!(id = %entry.id, "{error}"))
                    .ok();
                if let Some(collection) = &collection {
                    if !self.fingerprints.contains_key(&entry.id) {
                        collection.sweep_temp_files();
                        self.remember(&entry.id, collection);
                    }
                }
                tree_of(entry, collection.as_ref())
            })
            .collect()
    }

    /// Ids and roots of the collections that can be watched right now.
    pub fn watch_targets(&self) -> Vec<(String, PathBuf)> {
        self.registry
            .entries()
            .iter()
            .filter(|entry| open_registered(entry).is_ok())
            .map(|entry| (entry.id.clone(), entry.path.clone()))
            .collect()
    }

    pub fn create_collection(
        &mut self,
        parent: &Path,
        name: &str,
    ) -> Result<CollectionTree, AppError> {
        let collection = Collection::create(parent, name)?;
        self.register(collection)
    }

    /// Opens the folder at `path`; an already open folder returns its existing entry.
    pub fn open_collection(&mut self, path: &Path) -> Result<CollectionTree, AppError> {
        let canonical = fsio::canonicalize(path)?;
        let collection = Collection::open(&canonical)?;
        self.register(collection)
    }

    fn register(&mut self, collection: Collection) -> Result<CollectionTree, AppError> {
        let root = fsio::canonicalize(&collection.root)?;
        let entry = self.registry.add(&root)?;
        collection.sweep_temp_files();
        self.remember(&entry.id, &collection);
        Ok(tree_of(&entry, Some(&collection)))
    }

    /// Forgets the collection; its files are untouched.
    pub fn close_collection(&mut self, id: &str) -> Result<(), AppError> {
        self.registry.remove(id)?;
        self.fingerprints.remove(id);
        Ok(())
    }

    pub fn read_tree(&self, id: &str) -> Result<CollectionTree, AppError> {
        let entry = self.entry(id)?;
        let collection = open_registered(&entry).ok();
        Ok(tree_of(&entry, collection.as_ref()))
    }

    /// Runs one tree operation, then refreshes the fingerprint and returns the new tree.
    fn mutate(
        &mut self,
        id: &str,
        operation: impl FnOnce(&Collection) -> Result<Option<String>, AppError>,
    ) -> Result<TreeChange, AppError> {
        let (entry, collection) = self.open_entry(id)?;
        let result = operation(&collection);
        self.remember(id, &collection);
        let path = result?;
        Ok(TreeChange {
            tree: tree_of(&entry, Some(&collection)),
            path,
        })
    }

    pub fn create_folder(
        &mut self,
        id: &str,
        parent: &str,
        name: &str,
    ) -> Result<TreeChange, AppError> {
        self.mutate(id, |c| c.create_folder(parent, name).map(Some))
    }

    pub fn create_request(
        &mut self,
        id: &str,
        parent: &str,
        name: &str,
    ) -> Result<TreeChange, AppError> {
        self.mutate(id, |c| c.create_request(parent, name).map(Some))
    }

    pub fn rename_node(
        &mut self,
        id: &str,
        path: &str,
        name: &str,
    ) -> Result<TreeChange, AppError> {
        self.mutate(id, |c| c.rename(path, name).map(Some))
    }

    pub fn delete_node(&mut self, id: &str, path: &str) -> Result<TreeChange, AppError> {
        self.mutate(id, |c| c.delete(path).map(|()| None))
    }

    pub fn move_node(
        &mut self,
        id: &str,
        path: &str,
        target_folder: &str,
        index: u32,
    ) -> Result<TreeChange, AppError> {
        self.mutate(id, |c| c.move_node(path, target_folder, index).map(Some))
    }

    /// A collection request comes back with the secrets typed this session (plan 03 §4).
    pub fn read_request(&mut self, location: &RequestLocation) -> Result<RequestDoc, AppError> {
        match location {
            RequestLocation::Scratch { id } => doc(self.scratch.read(id)?),
            RequestLocation::Collection {
                collection_id,
                path,
            } => {
                let disk = doc(self.open_entry(collection_id)?.1.read_request(path)?)?;
                Ok(RequestDoc {
                    content: self.secrets.recall(location, disk.content),
                    name: disk.name,
                })
            }
        }
    }

    pub fn write_request(
        &mut self,
        location: &RequestLocation,
        content: &RequestContent,
    ) -> Result<(), AppError> {
        match location {
            RequestLocation::Scratch { id } => self.scratch.write(id, content),
            RequestLocation::Collection {
                collection_id,
                path,
            } => {
                let redacted = redact(content);
                self.mutate(collection_id, |c| {
                    c.write_request(path, &redacted).map(|()| None)
                })?;
                self.secrets.remember(location, normalise(content));
                Ok(())
            }
        }
    }

    pub fn list_scratch(&self) -> Result<Vec<ScratchItem>, AppError> {
        self.scratch.list()
    }

    pub fn create_scratch(&self) -> Result<ScratchItem, AppError> {
        self.scratch.create()
    }

    pub fn rename_scratch(&self, id: &str, name: &str) -> Result<Vec<ScratchItem>, AppError> {
        self.scratch.rename(id, name)
    }

    pub fn delete_scratch(&self, id: &str) -> Result<Vec<ScratchItem>, AppError> {
        self.scratch.delete(id)
    }

    /// Writes the scratch request into the collection, then removes the scratch file.
    pub fn move_scratch_to_collection(
        &mut self,
        id: &str,
        collection_id: &str,
        target_folder: &str,
    ) -> Result<TreeChange, AppError> {
        let file = self.scratch.read(id)?;
        let full = file.content()?;
        let redacted = file.with_content(&redact(&full))?;
        let change = self.mutate(collection_id, |c| {
            c.add_request(target_folder, &redacted).map(Some)
        })?;
        self.scratch.delete(id)?;
        if let Some(path) = &change.path {
            let location = RequestLocation::Collection {
                collection_id: collection_id.to_string(),
                path: path.clone(),
            };
            self.secrets.remember(&location, normalise(&full));
        }
        Ok(change)
    }

    /// The display name of the request at `location`, for history labels.
    pub fn request_name(&mut self, location: &RequestLocation) -> Option<String> {
        self.read_request(location).ok().map(|doc| doc.name)
    }

    pub fn append_history(&self, sent: &Sent<'_>) -> Result<(), AppError> {
        self.history.append(HistoryRecord::new(sent))
    }

    pub fn list_history(&self) -> Vec<HistoryEntry> {
        self.history.list()
    }

    pub fn clear_history(&self) -> Result<(), AppError> {
        self.history.clear()
    }

    /// A new scratch request from a history snapshot; collections are never touched.
    pub fn restore_history_entry(&self, id: &str) -> Result<ScratchItem, AppError> {
        let record = self.history.find(id)?;
        self.scratch
            .create_from(&record.entry.name, &record.content)
    }

    /// Recomputes the fingerprint; true when it differs from the stored one (an outside change).
    pub fn refresh_fingerprint(&mut self, id: &str) -> bool {
        let Ok((_, collection)) = self.open_entry(id) else {
            return self.fingerprints.remove(id).is_some();
        };
        let current = collection.fingerprint();
        self.fingerprints.insert(id.to_string(), current) != Some(current)
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
