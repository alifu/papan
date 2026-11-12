//! Tauri commands for sending requests and handling responses. Thin adapters over
//! [`HttpEngine`]; the store lock is never held across a network await.

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use super::types::{PickedFile, ResponseBody, ResponseMeta};
use super::variables::send_with_variables;
use super::HttpEngine;
use crate::error::AppError;
use crate::store::types::{RequestContent, RequestLocation};
use crate::store::StoreState;
use crate::vars::commands::VarsState;

type EngineResult<T> = Result<T, AppError>;

const DEFAULT_SAVE_NAME: &str = "response.bin";

fn logged<T>(result: EngineResult<T>) -> EngineResult<T> {
    if let Err(error) = &result {
        tracing::warn!(code = ?error.code, detail = ?error.detail, "{error}");
    }
    result
}

fn picked_path(picked: tauri_plugin_dialog::FilePath) -> EngineResult<std::path::PathBuf> {
    picked.into_path().map_err(|source| {
        AppError::invalid_input("That file cannot be used.").with_detail(source.to_string())
    })
}

/// Sends the draft `content`; `location` picks the cookie jar and labels history.
// Variables are resolved first, with the scopes of `location` (plan 04 §6).
#[tauri::command]
#[specta::specta]
pub async fn send_request(
    engine: State<'_, HttpEngine>,
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    request_id: String,
    location: RequestLocation,
    content: RequestContent,
) -> EngineResult<ResponseMeta> {
    let result = send_with_variables(
        &engine,
        &store,
        &vars.service,
        vars.dynamics.clone(),
        &request_id,
        &location,
        &content,
    )
    .await;
    logged(result)
}

/// Aborts an in-flight send. An unknown id is a no-op.
#[tauri::command]
#[specta::specta]
pub fn cancel_request(engine: State<'_, HttpEngine>, request_id: String) -> EngineResult<()> {
    logged(engine.cancel(&request_id))
}

/// The capped body preview; `not_found` once the response file is evicted.
#[tauri::command]
#[specta::specta]
pub async fn read_response_body(
    engine: State<'_, HttpEngine>,
    request_id: String,
) -> EngineResult<ResponseBody> {
    logged(engine.read_body(&request_id))
}

/// Runs a blocking native dialog off the async runtime.
async fn dialog<T: Send + 'static>(show: impl FnOnce() -> T + Send + 'static) -> EngineResult<T> {
    tauri::async_runtime::spawn_blocking(show)
        .await
        .map_err(|source| {
            AppError::internal("The file dialog failed.").with_detail(source.to_string())
        })
}

/// Saves the full body through the native save dialog; `None` when cancelled.
#[tauri::command]
#[specta::specta]
pub async fn save_response_body(
    app: AppHandle,
    engine: State<'_, HttpEngine>,
    request_id: String,
) -> EngineResult<Option<String>> {
    let source = logged(engine.body_file(&request_id))?;
    let picked = dialog(move || {
        app.dialog()
            .file()
            .set_file_name(DEFAULT_SAVE_NAME)
            .blocking_save_file()
    })
    .await?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let target = logged(picked_path(picked))?;
    logged(
        tokio::fs::copy(&source, &target)
            .await
            .map_err(AppError::from),
    )?;
    Ok(target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned()))
}

/// Lets the user pick a file to upload and grants it to the request at `location`;
/// `None` when cancelled.
#[tauri::command]
#[specta::specta]
pub async fn pick_body_file(
    app: AppHandle,
    engine: State<'_, HttpEngine>,
    location: RequestLocation,
) -> EngineResult<Option<PickedFile>> {
    let picked = dialog(move || app.dialog().file().blocking_pick_file()).await?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = logged(picked_path(picked))?;
    logged(engine.grant(&location, &path)).map(Some)
}

/// Clears the cookie jar that `location` uses.
#[tauri::command]
#[specta::specta]
pub fn clear_cookies(engine: State<'_, HttpEngine>, location: RequestLocation) -> EngineResult<()> {
    engine.clear_cookies(&location);
    Ok(())
}
