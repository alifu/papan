//! Tauri commands for collections and scratch requests. Thin adapters: lock, call the
//! plain-Rust `Store`, log failures. Watchers start on list/open/create and stop on close.

use std::path::PathBuf;

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_specta::Event;

use super::types::{
    CollectionChanged, CollectionTree, RequestContent, RequestDoc, RequestLocation, ScratchItem,
    TreeChange,
};
use super::{model, watcher, Store, StoreState};
use crate::error::AppError;

type StoreResult<T> = Result<T, AppError>;

fn logged<T>(result: StoreResult<T>) -> StoreResult<T> {
    if let Err(error) = &result {
        tracing::warn!(code = ?error.code, detail = ?error.detail, "{error}");
    }
    result
}

fn with_store<T>(
    state: &StoreState,
    operation: impl FnOnce(&mut Store) -> StoreResult<T>,
) -> StoreResult<T> {
    logged(operation(&mut state.lock_store()))
}

/// Starts a watcher for every watchable collection that has none yet.
fn sync_watchers(state: &StoreState, app: &AppHandle) {
    let targets = state.lock_store().watch_targets();
    let mut watchers = state.lock_watchers();
    for (id, root) in targets {
        if watchers.contains_key(&id) {
            continue;
        }
        let emitter = app.clone();
        let on_change = move |collection_id: &str| {
            let event = CollectionChanged {
                collection_id: collection_id.to_string(),
            };
            if let Err(error) = event.emit(&emitter) {
                tracing::warn!(%error, "could not emit collectionChanged");
            }
        };
        match watcher::watch(&root, id.clone(), state.store.clone(), on_change) {
            Ok(handle) => {
                watchers.insert(id, handle);
            }
            Err(error) => tracing::warn!(detail = ?error.detail, "{error}"),
        }
    }
}

/// Shows the native folder picker; `None` when the user cancels.
fn pick_folder(app: &AppHandle) -> StoreResult<Option<PathBuf>> {
    app.dialog()
        .file()
        .blocking_pick_folder()
        .map(|picked| {
            picked.into_path().map_err(|source| {
                AppError::invalid_input("That folder cannot be used.")
                    .with_detail(source.to_string())
            })
        })
        .transpose()
}

#[tauri::command]
#[specta::specta]
pub fn list_collections(
    state: State<'_, StoreState>,
    app: AppHandle,
) -> StoreResult<Vec<CollectionTree>> {
    let trees = with_store(&state, |store| Ok(store.list_collections()))?;
    sync_watchers(&state, &app);
    Ok(trees)
}

#[tauri::command]
#[specta::specta]
pub async fn create_collection(
    state: State<'_, StoreState>,
    app: AppHandle,
    name: String,
) -> StoreResult<Option<CollectionTree>> {
    logged(model::clean_name(&name))?;
    let Some(parent) = logged(pick_folder(&app))? else {
        return Ok(None);
    };
    let tree = with_store(&state, |store| store.create_collection(&parent, &name))?;
    sync_watchers(&state, &app);
    Ok(Some(tree))
}

#[tauri::command]
#[specta::specta]
pub async fn open_collection(
    state: State<'_, StoreState>,
    app: AppHandle,
) -> StoreResult<Option<CollectionTree>> {
    let Some(folder) = logged(pick_folder(&app))? else {
        return Ok(None);
    };
    let tree = with_store(&state, |store| store.open_collection(&folder))?;
    sync_watchers(&state, &app);
    Ok(Some(tree))
}

#[tauri::command]
#[specta::specta]
pub fn close_collection(state: State<'_, StoreState>, collection_id: String) -> StoreResult<()> {
    with_store(&state, |store| store.close_collection(&collection_id))?;
    // Dropped after the store lock is released: the watcher's callback takes that lock.
    let stopped = state.lock_watchers().remove(&collection_id);
    drop(stopped);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn read_tree(
    state: State<'_, StoreState>,
    collection_id: String,
) -> StoreResult<CollectionTree> {
    with_store(&state, |store| store.read_tree(&collection_id))
}

#[tauri::command]
#[specta::specta]
pub fn create_folder(
    state: State<'_, StoreState>,
    collection_id: String,
    parent: String,
    name: String,
) -> StoreResult<TreeChange> {
    with_store(&state, |store| {
        store.create_folder(&collection_id, &parent, &name)
    })
}

#[tauri::command]
#[specta::specta]
pub fn create_request(
    state: State<'_, StoreState>,
    collection_id: String,
    parent: String,
    name: String,
) -> StoreResult<TreeChange> {
    with_store(&state, |store| {
        store.create_request(&collection_id, &parent, &name)
    })
}

#[tauri::command]
#[specta::specta]
pub fn rename_node(
    state: State<'_, StoreState>,
    collection_id: String,
    path: String,
    name: String,
) -> StoreResult<TreeChange> {
    with_store(&state, |store| {
        store.rename_node(&collection_id, &path, &name)
    })
}

#[tauri::command]
#[specta::specta]
pub fn delete_node(
    state: State<'_, StoreState>,
    collection_id: String,
    path: String,
) -> StoreResult<TreeChange> {
    with_store(&state, |store| store.delete_node(&collection_id, &path))
}

#[tauri::command]
#[specta::specta]
pub fn move_node(
    state: State<'_, StoreState>,
    collection_id: String,
    path: String,
    target_folder: String,
    index: u32,
) -> StoreResult<TreeChange> {
    with_store(&state, |store| {
        store.move_node(&collection_id, &path, &target_folder, index)
    })
}

#[tauri::command]
#[specta::specta]
pub fn read_request(
    state: State<'_, StoreState>,
    location: RequestLocation,
) -> StoreResult<RequestDoc> {
    with_store(&state, |store| store.read_request(&location))
}

#[tauri::command]
#[specta::specta]
pub fn write_request(
    state: State<'_, StoreState>,
    location: RequestLocation,
    content: RequestContent,
) -> StoreResult<()> {
    with_store(&state, |store| store.write_request(&location, &content))
}

#[tauri::command]
#[specta::specta]
pub fn list_scratch(state: State<'_, StoreState>) -> StoreResult<Vec<ScratchItem>> {
    with_store(&state, |store| store.list_scratch())
}

#[tauri::command]
#[specta::specta]
pub fn create_scratch(state: State<'_, StoreState>) -> StoreResult<ScratchItem> {
    with_store(&state, |store| store.create_scratch())
}

#[tauri::command]
#[specta::specta]
pub fn rename_scratch(
    state: State<'_, StoreState>,
    id: String,
    name: String,
) -> StoreResult<Vec<ScratchItem>> {
    with_store(&state, |store| store.rename_scratch(&id, &name))
}

#[tauri::command]
#[specta::specta]
pub fn delete_scratch(state: State<'_, StoreState>, id: String) -> StoreResult<Vec<ScratchItem>> {
    with_store(&state, |store| store.delete_scratch(&id))
}

#[tauri::command]
#[specta::specta]
pub fn move_scratch_to_collection(
    state: State<'_, StoreState>,
    id: String,
    collection_id: String,
    target_folder: String,
) -> StoreResult<TreeChange> {
    with_store(&state, |store| {
        store.move_scratch_to_collection(&id, &collection_id, &target_folder)
    })
}
