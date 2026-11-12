//! Collections and scratch requests as files on disk (plan 02-storage).
//!
//! `types` is the IPC contract and `commands` the thin Tauri adapters; everything else is
//! plain Rust. Filesystem calls live only in `fsio` and `paths`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

pub mod collection;
pub mod commands;
pub mod fingerprint;
pub mod fsio;
pub mod model;
pub mod paths;
pub mod registry;
pub mod scan;
pub mod scratch;
pub mod service;
pub mod tree_ops;
pub mod types;
pub mod watcher;
pub mod yaml;

pub use service::Store;

/// Managed by Tauri. One lock around the store; watchers are kept apart so a watcher is
/// never dropped while the store lock is held (its callback takes that lock).
#[derive(Debug)]
pub struct StoreState {
    pub store: Arc<Mutex<Store>>,
    pub watchers: Mutex<HashMap<String, watcher::CollectionWatcher>>,
}

impl StoreState {
    pub fn new(store: Store) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
            watchers: Mutex::new(HashMap::new()),
        }
    }

    /// The store, recovered if an earlier holder panicked: every store operation leaves
    /// the disk consistent on its own, so the data behind the lock is still usable.
    pub fn lock_store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn lock_watchers(&self) -> MutexGuard<'_, HashMap<String, watcher::CollectionWatcher>> {
        self.watchers.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_poisoned_lock_is_recovered() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(StoreState::new(Store::new(dir.path()).unwrap()));
        let poisoner = Arc::clone(&state);

        let panicked = std::thread::spawn(move || {
            let _guard = poisoner.lock_store();
            panic!("poison the lock");
        })
        .join();

        assert!(panicked.is_err());
        assert!(state.store.is_poisoned());
        assert!(state.lock_store().list_scratch().unwrap().is_empty());
        assert!(state.lock_watchers().is_empty());
    }
}
