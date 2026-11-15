//! Tauri commands for mocks (plan 11 §6). Thin adapters over `disk` and `MockService`.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_specta::Event;

use super::model::MockEnvironment;
use super::types::{MockLogEntry, MockLogged, MockSummary};
use super::{disk, MockService, MockSink};
use crate::error::AppError;
use crate::store::StoreState;

type MockResult<T> = Result<T, AppError>;

/// Forwards each served request to the webview as `mockLogged`.
pub struct TauriSink {
    app: AppHandle,
}

impl TauriSink {
    pub fn arc(app: AppHandle) -> Arc<dyn MockSink> {
        Arc::new(Self { app })
    }
}

impl MockSink for TauriSink {
    fn logged(&self, collection_id: &str, file: &str, entry: &MockLogEntry) {
        let event = MockLogged {
            collection_id: collection_id.to_string(),
            file: file.to_string(),
            entry: entry.clone(),
        };
        if let Err(source) = event.emit(&self.app) {
            tracing::warn!(%source, "could not emit mockLogged");
        }
    }
}

fn root_of(state: &StoreState, collection_id: &str) -> MockResult<PathBuf> {
    state.lock_store().collection_root(collection_id)
}

/// Mocks from a cloned collection may not expose the network or read files until the user
/// trusts the collection (the same switch as for its scripts).
fn check_trust(state: &StoreState, collection_id: &str, mock: &MockEnvironment) -> MockResult<()> {
    let Some(reason) = super::risky_features(mock) else {
        return Ok(());
    };
    if state.lock_store().scripts_trusted(collection_id) {
        return Ok(());
    }
    Err(AppError::invalid_input(format!(
        "This mock {reason}. It is in a collection that came from a Git repository, so it will not start until you trust the collection (collection menu, Trust scripts…)."
    )))
}

fn logged<T>(result: MockResult<T>) -> MockResult<T> {
    if let Err(error) = &result {
        tracing::warn!(code = ?error.code, detail = ?error.detail, "{error}");
    }
    result
}

async fn with_state(
    service: &MockService,
    collection_id: &str,
    mut summary: MockSummary,
) -> MockSummary {
    if let Some(url) = service.url_of(collection_id, &summary.file).await {
        summary.running = true;
        summary.url = Some(url);
    }
    summary
}

#[tauri::command]
#[specta::specta]
pub async fn list_mocks(
    state: State<'_, StoreState>,
    service: State<'_, MockService>,
    collection_id: String,
) -> MockResult<Vec<MockSummary>> {
    let root = root_of(&state, &collection_id)?;
    let listed = tauri::async_runtime::spawn_blocking(move || disk::list(&root))
        .await
        .map_err(|source| {
            AppError::internal("Could not list mocks.").with_detail(source.to_string())
        })?;
    let mut out = Vec::with_capacity(listed.len());
    for summary in listed {
        out.push(with_state(&service, &collection_id, summary).await);
    }
    Ok(out)
}

#[tauri::command]
#[specta::specta]
pub fn read_mock(
    state: State<'_, StoreState>,
    collection_id: String,
    file: String,
) -> MockResult<MockEnvironment> {
    let root = root_of(&state, &collection_id)?;
    logged(disk::read(&root, &file))
}

/// Saves a mock (a new file when `file` is null). A running mock restarts with the new
/// definition.
#[tauri::command]
#[specta::specta]
pub async fn save_mock(
    state: State<'_, StoreState>,
    service: State<'_, MockService>,
    collection_id: String,
    file: Option<String>,
    mock: MockEnvironment,
) -> MockResult<MockSummary> {
    let root = root_of(&state, &collection_id)?;
    let saved = mock.clone();
    let name = logged(disk::write(&root, file.as_deref(), &mock))?;
    if service.url_of(&collection_id, &name).await.is_some() {
        logged(check_trust(&state, &collection_id, &saved))?;
        service.stop(&collection_id, &name).await;
        logged(
            service
                .start(&collection_id, &name, root, saved.clone())
                .await,
        )?;
    }
    Ok(with_state(&service, &collection_id, MockSummary::of(&name, &saved)).await)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_mock(
    state: State<'_, StoreState>,
    service: State<'_, MockService>,
    collection_id: String,
    file: String,
) -> MockResult<()> {
    let root = root_of(&state, &collection_id)?;
    service.stop(&collection_id, &file).await;
    logged(disk::delete(&root, &file))
}

/// Starts the mock and returns its URL.
#[tauri::command]
#[specta::specta]
pub async fn start_mock(
    state: State<'_, StoreState>,
    service: State<'_, MockService>,
    collection_id: String,
    file: String,
) -> MockResult<String> {
    let root = root_of(&state, &collection_id)?;
    let mock = logged(disk::read(&root, &file))?;
    logged(check_trust(&state, &collection_id, &mock))?;
    logged(service.start(&collection_id, &file, root, mock).await)?;
    service
        .url_of(&collection_id, &file)
        .await
        .ok_or_else(|| AppError::internal("The mock stopped right after it started."))
}

#[tauri::command]
#[specta::specta]
pub async fn stop_mock(
    service: State<'_, MockService>,
    collection_id: String,
    file: String,
) -> MockResult<()> {
    service.stop(&collection_id, &file).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn mock_log(
    service: State<'_, MockService>,
    collection_id: String,
    file: String,
) -> MockResult<Vec<MockLogEntry>> {
    Ok(service.log(&collection_id, &file).await)
}

#[tauri::command]
#[specta::specta]
pub async fn clear_mock_log(
    service: State<'_, MockService>,
    collection_id: String,
    file: String,
) -> MockResult<()> {
    service.clear_log(&collection_id, &file).await;
    Ok(())
}
