//! Tauri commands for Git. Thin adapters: resolve the collection folder, run the blocking
//! `git` work off the UI thread, log failures.

use std::path::PathBuf;

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use super::service;
use super::types::{ConflictSide, GitBranch, GitOutput, GitStatus};
use crate::error::AppError;
use crate::store::types::CollectionTree;
use crate::store::StoreState;

type GitResult<T> = Result<T, AppError>;

fn logged<T>(result: GitResult<T>) -> GitResult<T> {
    if let Err(error) = &result {
        tracing::warn!(code = ?error.code, detail = ?error.detail, "{error}");
    }
    result
}

fn root_of(state: &StoreState, collection_id: &str) -> GitResult<PathBuf> {
    state.lock_store().collection_root(collection_id)
}

/// Runs blocking git work on the blocking pool.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> GitResult<T> + Send + 'static,
) -> GitResult<T> {
    let joined = tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|source| {
            AppError::internal("Git work failed to finish.").with_detail(source.to_string())
        })?;
    logged(joined)
}

#[tauri::command]
#[specta::specta]
pub async fn git_status(
    state: State<'_, StoreState>,
    collection_id: String,
) -> GitResult<GitStatus> {
    let root = root_of(&state, &collection_id)?;
    blocking(move || service::status(&root)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_init(state: State<'_, StoreState>, collection_id: String) -> GitResult<GitStatus> {
    let root = root_of(&state, &collection_id)?;
    blocking(move || service::init(&root)).await
}

/// Asks for a parent folder, clones into it and opens the clone as a collection.
/// `None` when the user cancels the folder picker. A clone that is not a Papan
/// collection is removed again (only the folder this call created).
#[tauri::command]
#[specta::specta]
pub async fn git_clone(
    state: State<'_, StoreState>,
    app: AppHandle,
    url: String,
) -> GitResult<Option<CollectionTree>> {
    let picked =
        tauri::async_runtime::spawn_blocking(move || app.dialog().file().blocking_pick_folder())
            .await
            .map_err(|source| {
                AppError::internal("The folder picker failed.").with_detail(source.to_string())
            })?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let parent = picked.into_path().map_err(|source| {
        AppError::invalid_input("That folder cannot be used.").with_detail(source.to_string())
    })?;
    let cloned = blocking(move || service::clone(&url, &parent)).await?;
    let opened = logged(state.lock_store().open_collection(&cloned));
    match opened {
        Ok(tree) => Ok(Some(tree)),
        Err(error) => {
            if let Err(cleanup) = std::fs::remove_dir_all(&cloned) {
                tracing::warn!(%cleanup, "could not remove the rejected clone");
            }
            Err(
                AppError::invalid_input("That repository is not a Papan collection.")
                    .with_detail(format!("{} ({:?})", error.message, error.detail)),
            )
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn git_diff(
    state: State<'_, StoreState>,
    collection_id: String,
    path: String,
) -> GitResult<String> {
    let root = root_of(&state, &collection_id)?;
    blocking(move || service::diff(&root, &path)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_commit(
    state: State<'_, StoreState>,
    collection_id: String,
    message: String,
) -> GitResult<GitOutput> {
    let root = root_of(&state, &collection_id)?;
    blocking(move || service::commit(&root, &message)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_pull(state: State<'_, StoreState>, collection_id: String) -> GitResult<GitOutput> {
    let root = root_of(&state, &collection_id)?;
    blocking(move || service::pull(&root)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_push(state: State<'_, StoreState>, collection_id: String) -> GitResult<GitOutput> {
    let root = root_of(&state, &collection_id)?;
    blocking(move || service::push(&root)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_branches(
    state: State<'_, StoreState>,
    collection_id: String,
) -> GitResult<Vec<GitBranch>> {
    let root = root_of(&state, &collection_id)?;
    blocking(move || service::branches(&root)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_switch(
    state: State<'_, StoreState>,
    collection_id: String,
    branch: String,
) -> GitResult<GitStatus> {
    let root = root_of(&state, &collection_id)?;
    blocking(move || service::switch(&root, &branch)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_create_branch(
    state: State<'_, StoreState>,
    collection_id: String,
    branch: String,
) -> GitResult<GitStatus> {
    let root = root_of(&state, &collection_id)?;
    blocking(move || service::create_branch(&root, &branch)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_resolve(
    state: State<'_, StoreState>,
    collection_id: String,
    path: String,
    side: ConflictSide,
) -> GitResult<GitStatus> {
    let root = root_of(&state, &collection_id)?;
    blocking(move || service::resolve(&root, &path, side)).await
}
