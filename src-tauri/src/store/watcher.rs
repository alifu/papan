//! Watches one collection for outside changes (plan §5). No Tauri types: the caller
//! supplies `on_change`. Our own writes leave the fingerprint unchanged, so they are
//! filtered out without timers or path allow-lists.

use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Duration;

use notify_debouncer_mini::notify::{self, RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer_opt, Config, DebounceEventResult, Debouncer};

use super::collection::REQUESTS_DIR;
use super::environments::ENVIRONMENTS_DIR;
use super::fsio::{self, EntryKind};
use super::service::Store;
use crate::error::AppError;

const DEBOUNCE: Duration = Duration::from_millis(300);

/// Stops watching when dropped.
pub struct CollectionWatcher {
    _debouncer: Shared,
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

type Shared = Arc<Mutex<Option<Debouncer<RecommendedWatcher>>>>;

/// Subfolders watched once they exist: `git pull` may create them after the watcher
/// started, so the handler keeps checking for the ones still missing.
fn subfolders(root: &Path) -> Vec<(PathBuf, RecursiveMode)> {
    vec![
        (root.join(ENVIRONMENTS_DIR), RecursiveMode::NonRecursive),
        (root.join(REQUESTS_DIR), RecursiveMode::Recursive),
    ]
}

fn is_dir(path: &Path) -> bool {
    fsio::entry_kind(path) == Some(EntryKind::Dir)
}

/// Watches every pending subfolder that now exists; keeps the rest pending.
fn watch_new_subfolders(
    shared: &Weak<Mutex<Option<Debouncer<RecommendedWatcher>>>>,
    pending: &mut Vec<(PathBuf, RecursiveMode)>,
) {
    if !pending.iter().any(|(path, _)| is_dir(path)) {
        return;
    }
    let Some(shared) = shared.upgrade() else {
        return;
    };
    let mut guard = shared.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(debouncer) = guard.as_mut() else {
        return;
    };
    pending.retain(|(path, mode)| {
        if !is_dir(path) {
            return true;
        }
        match debouncer.watcher().watch(path, *mode) {
            Ok(()) => false,
            Err(error) => {
                tracing::warn!(%error, "could not watch a new collection folder");
                true
            }
        }
    });
}

/// Watches `root` and `environments/` (non-recursively) and `requests/` (recursively);
/// a subfolder created later is watched from the first event that shows it. After each
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
    let (existing, mut pending): (Vec<_>, Vec<_>) = subfolders(root)
        .into_iter()
        .partition(|(path, _)| is_dir(path));
    let shared: Shared = Arc::new(Mutex::new(None));
    let weak = Arc::downgrade(&shared);
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
        watch_new_subfolders(&weak, &mut pending);
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
    for (path, mode) in existing {
        watcher.watch(&path, mode).map_err(watch_error)?;
    }
    *shared.lock().unwrap_or_else(PoisonError::into_inner) = Some(debouncer);
    Ok(CollectionWatcher { _debouncer: shared })
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
