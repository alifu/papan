//! The cookie jars of an engine and their persistence (plan 03b §6.5, §15.5.6).
//!
//! A location's verified jar is loaded from its file on first use and saved, debounced,
//! after a send or a manager edit that changed its persistent cookies. The
//! `#tls-unverified` twin is never loaded nor saved. Without [`Persisted`] (no vault
//! given to the engine) jars live in memory only.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use reqwest_cookie_store::{CookieStore, CookieStoreMutex};

use super::client::{jar_key, Clients};
use super::cookie_file;
use super::cookies;
use super::types::{CookieFilter, CookieInput, StoredCookie};
use crate::error::AppError;
use crate::store::types::RequestLocation;
use crate::vars::vault::SecretStore;

/// Edits within this delay are written once.
const SAVE_DELAY: Duration = Duration::from_secs(1);
const COLLECTION_JAR: &str = "collection:";
const SCRATCH_JAR: &str = "scratch";

/// Where jars are written and the vault holding their key.
#[derive(Clone)]
pub struct Persisted {
    pub dir: PathBuf,
    pub vault: Arc<dyn SecretStore>,
}

pub struct CookieJars {
    clients: Arc<Clients>,
    persisted: Option<Persisted>,
    /// Jars already read from disk (or cleared), by jar key.
    loaded: Mutex<HashSet<String>>,
    /// Jars whose file could not be read for now (vault failed or denied): never saved
    /// over until a later read succeeds or the jar is cleared.
    unreadable: Mutex<HashSet<String>>,
    /// Bumped by every clear. A save writes only under this lock and only if the epoch
    /// it started with is still current, so a cleared jar never comes back from disk.
    epochs: Arc<Mutex<HashMap<String, u64>>>,
    /// Jars with a save scheduled.
    pending: Mutex<HashSet<String>>,
    /// The persistent cookies last read or written, by jar key.
    saved: Mutex<HashMap<String, Vec<StoredCookie>>>,
}

impl std::fmt::Debug for CookieJars {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CookieJars")
            .field("persisted", &self.persisted.as_ref().map(|p| &p.dir))
            .finish_non_exhaustive()
    }
}

fn recover<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}

fn lock_store(store: &CookieStoreMutex) -> MutexGuard<'_, CookieStore> {
    store.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The file name of a verified jar; `None` for anything else (the unverified twin).
fn file_name(jar: &str) -> Option<String> {
    if jar == SCRATCH_JAR {
        return Some(SCRATCH_JAR.to_string());
    }
    jar.strip_prefix(COLLECTION_JAR)
        .filter(|id| !id.contains('#'))
        .map(|id| format!("collection-{id}"))
}

fn persistent(store: &CookieStore) -> Vec<StoredCookie> {
    cookies::list(store)
        .into_iter()
        .filter(|cookie| cookie.expires_at_ms.is_some())
        .collect()
}

fn epoch_of(epochs: &HashMap<String, u64>, jar: &str) -> u64 {
    epochs.get(jar).copied().unwrap_or_default()
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, AppError> {
    tokio::task::spawn_blocking(work).await.map_err(|source| {
        AppError::internal("Could not access the cookie jar.").with_detail(source.to_string())
    })
}

impl CookieJars {
    pub fn new(clients: Arc<Clients>, persisted: Option<Persisted>) -> Self {
        Self {
            clients,
            persisted,
            loaded: Mutex::default(),
            unreadable: Mutex::default(),
            epochs: Arc::default(),
            pending: Mutex::default(),
            saved: Mutex::default(),
        }
    }

    /// The verified jar `jar`, read from its file on first use (off the async runtime).
    pub(super) async fn open(&self, jar: &str) -> Result<Arc<CookieStoreMutex>, AppError> {
        let store = self.clients.jar(jar);
        let (Some(persisted), Some(name)) = (self.persisted.clone(), file_name(jar)) else {
            return Ok(store);
        };
        if recover(&self.loaded).contains(jar) {
            return Ok(store);
        }
        let read =
            blocking(move || cookie_file::load(&persisted.dir, &name, &*persisted.vault)).await?;
        let from_file = match read {
            Ok(from_file) => from_file,
            Err(error) => {
                // Sends go on with an in-memory jar; the file is read again next time.
                tracing::warn!(code = ?error.code, "cookie jar not read: {error}");
                recover(&self.unreadable).insert(jar.to_string());
                return Ok(store);
            }
        };
        {
            let mut current = lock_store(&store);
            if current.iter_any().next().is_none() {
                if let Some(from_file) = from_file {
                    recover(&self.saved).insert(jar.to_string(), persistent(&from_file));
                    *current = from_file;
                }
                recover(&self.unreadable).remove(jar);
            }
            // Otherwise cookies arrived while the file was unreadable: a jar still marked
            // unreadable keeps the file as it is for the rest of the session.
        }
        recover(&self.loaded).insert(jar.to_string());
        Ok(store)
    }

    /// Schedules a save of the verified jar `jar` after [`SAVE_DELAY`].
    pub(super) fn changed(self: &Arc<Self>, jar: &str) {
        if self.persisted.is_none() || file_name(jar).is_none() {
            return;
        }
        if !recover(&self.pending).insert(jar.to_string()) {
            return;
        }
        let (jars, jar) = (Arc::clone(self), jar.to_string());
        tokio::spawn(async move {
            tokio::time::sleep(SAVE_DELAY).await;
            jars.save(&jar).await;
        });
    }

    /// Writes `jar` now if its persistent cookies changed since the last read or write.
    async fn save(&self, jar: &str) {
        recover(&self.pending).remove(jar);
        let (Some(persisted), Some(name)) = (self.persisted.clone(), file_name(jar)) else {
            return;
        };
        if recover(&self.unreadable).contains(jar) {
            return;
        }
        let epoch = epoch_of(&recover(&self.epochs), jar);
        let snapshot = lock_store(&self.clients.jar(jar)).clone();
        let now = persistent(&snapshot);
        if recover(&self.saved)
            .get(jar)
            .map_or(now.is_empty(), |saved| *saved == now)
        {
            return;
        }
        let (epochs, key) = (Arc::clone(&self.epochs), jar.to_string());
        let written = blocking(move || {
            let write = cookie_file::prepare(&persisted.dir, &name, &*persisted.vault, &snapshot)?;
            let epochs = recover(&epochs);
            if epoch_of(&epochs, &key) != epoch {
                return Ok(false);
            }
            write.commit().map(|()| true)
        })
        .await
        .and_then(|result| result);
        match written {
            Ok(true) => {
                recover(&self.saved).insert(jar.to_string(), now);
            }
            Ok(false) => tracing::debug!("cookie jar save dropped: cleared meanwhile"),
            Err(error) => tracing::warn!(code = ?error.code, "cookie jar not saved: {error}"),
        }
    }

    /// Writes every jar with a save scheduled (tests; before quitting).
    pub async fn flush(&self) {
        let pending: Vec<String> = recover(&self.pending).iter().cloned().collect();
        for jar in pending {
            self.save(&jar).await;
        }
    }

    /// Empties both jars of `jar` and deletes its file.
    pub(super) fn clear(&self, jar: &str) {
        let mut epochs = recover(&self.epochs);
        *epochs.entry(jar.to_string()).or_default() += 1;
        self.clients.clear(jar);
        recover(&self.loaded).insert(jar.to_string());
        recover(&self.unreadable).remove(jar);
        recover(&self.saved).insert(jar.to_string(), Vec::new());
        if let (Some(persisted), Some(name)) = (&self.persisted, file_name(jar)) {
            if let Err(error) = cookie_file::delete(&persisted.dir, &name) {
                tracing::warn!(code = ?error.code, "cookie jar file not deleted: {error}");
            }
        }
    }

    /// The unexpired cookies of the jar `location` uses, sorted by domain, path, name.
    pub async fn list(&self, location: &RequestLocation) -> Result<Vec<StoredCookie>, AppError> {
        let store = self.open(&jar_key(location)).await?;
        let listed = cookies::list(&lock_store(&store));
        Ok(listed)
    }

    /// Adds or replaces one cookie; returns the jar's new list.
    pub async fn set(
        self: &Arc<Self>,
        location: &RequestLocation,
        cookie: &CookieInput,
    ) -> Result<Vec<StoredCookie>, AppError> {
        self.edit(location, |store| cookies::set(store, cookie))
            .await
    }

    /// Deletes the cookies `filter` names; returns the jar's new list.
    pub async fn delete(
        self: &Arc<Self>,
        location: &RequestLocation,
        filter: &CookieFilter,
    ) -> Result<Vec<StoredCookie>, AppError> {
        self.edit(location, |store| cookies::delete(store, filter).map(|_| ()))
            .await
    }

    async fn edit(
        self: &Arc<Self>,
        location: &RequestLocation,
        change: impl FnOnce(&mut CookieStore) -> Result<(), AppError>,
    ) -> Result<Vec<StoredCookie>, AppError> {
        let jar = jar_key(location);
        let store = self.open(&jar).await?;
        let listed = {
            let mut guard = lock_store(&store);
            change(&mut guard)?;
            cookies::list(&guard)
        };
        self.changed(&jar);
        Ok(listed)
    }
}

#[cfg(test)]
#[path = "cookie_jars_tests.rs"]
mod tests;
