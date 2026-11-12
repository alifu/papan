//! Watches one collection for outside changes (plan §5). No Tauri types: the caller
//! supplies `on_change`. Our own writes leave the fingerprint unchanged, so they are
//! filtered out without timers or path allow-lists.

use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use notify_debouncer_mini::notify::{self, RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer_opt, Config, DebounceEventResult, Debouncer};

use super::collection::REQUESTS_DIR;
use super::fsio::{self, EntryKind};
use super::service::Store;
use crate::error::AppError;

const DEBOUNCE: Duration = Duration::from_millis(300);

/// Stops watching when dropped.
pub struct CollectionWatcher {
    _debouncer: Debouncer<RecommendedWatcher>,
}

impl std::fmt::Debug for CollectionWatcher {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CollectionWatcher")
    }
}

fn watch_error(source: impl std::fmt::Display) -> AppError {
    AppError::internal("Could not watch the collection for changes.")
        .with_detail(source.to_string())
}

/// True for paths inside a watched root that do not lie under a dot-named component
/// (`.git/`, our temp files). Paths outside the roots are never relevant.
fn is_relevant(roots: &[PathBuf], path: &Path) -> bool {
    let relative = roots.iter().find_map(|root| path.strip_prefix(root).ok());
    relative.is_some_and(|rel| {
        !rel.components().any(|component| match component {
            Component::Normal(name) => name.to_string_lossy().starts_with('.'),
            _ => false,
        })
    })
}

/// Watches `root` (non-recursively) and its `requests/` (recursively). After each
/// debounced burst the store's fingerprint for `id` is recomputed; `on_change(id)` runs
/// only when it changed.
pub fn watch(
    root: &Path,
    id: String,
    store: Arc<Mutex<Store>>,
    on_change: impl Fn(&str) + Send + 'static,
) -> Result<CollectionWatcher, AppError> {
    let roots: Vec<PathBuf> = std::iter::once(root.to_path_buf())
        .chain(fsio::canonicalize(root).ok())
        .collect();
    let handler = move |result: DebounceEventResult| {
        let events = match result {
            Ok(events) => events,
            Err(error) => {
                tracing::warn!(%error, "collection watcher error");
                return;
            }
        };
        if !events.iter().any(|event| is_relevant(&roots, &event.path)) {
            return;
        }
        let changed = store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .refresh_fingerprint(&id);
        if changed {
            on_change(&id);
        }
    };
    let config = Config::default()
        .with_timeout(DEBOUNCE)
        .with_notify_config(notify::Config::default().with_follow_symlinks(false));
    let mut debouncer =
        new_debouncer_opt::<_, RecommendedWatcher>(config, handler).map_err(watch_error)?;
    let watcher = debouncer.watcher();
    watcher
        .watch(root, RecursiveMode::NonRecursive)
        .map_err(watch_error)?;
    let requests = root.join(REQUESTS_DIR);
    if fsio::entry_kind(&requests) == Some(EntryKind::Dir) {
        watcher
            .watch(&requests, RecursiveMode::Recursive)
            .map_err(watch_error)?;
    }
    Ok(CollectionWatcher {
        _debouncer: debouncer,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_components_are_irrelevant() {
        let roots = vec![PathBuf::from("/c")];

        assert!(is_relevant(&roots, Path::new("/c/requests/a.http.yaml")));
        assert!(!is_relevant(&roots, Path::new("/c/.git/index")));
        assert!(!is_relevant(&roots, Path::new("/c/requests/.papan-tmp-x")));
        assert!(!is_relevant(&roots, Path::new("/elsewhere/a.http.yaml")));
    }
}
