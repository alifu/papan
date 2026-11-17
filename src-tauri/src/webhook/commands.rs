//! Tauri commands for webhook listeners (plan 07 §7). Thin adapters over `WebhookService`.

use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_specta::Event;

use super::types::{
    Capture, CaptureSummary, ReplayResult, WebhookCaptured, WebhookConfig, WebhookListener,
    WebhookStopped,
};
use super::{CaptureSink, WebhookService};
use crate::error::AppError;
use crate::store::scratch::Scratch;
use crate::store::StoreState;

type WebhookResult<T> = Result<T, AppError>;

/// Forwards each capture to the webview as `webhookCaptured`.
pub struct TauriSink {
    app: AppHandle,
}

impl TauriSink {
    pub fn arc(app: AppHandle) -> Arc<dyn CaptureSink> {
        Arc::new(Self { app })
    }
}

impl CaptureSink for TauriSink {
    fn captured(&self, listener_id: &str, capture: &CaptureSummary) {
        let event = WebhookCaptured {
            listener_id: listener_id.to_string(),
            capture: capture.clone(),
        };
        if let Err(source) = event.emit(&self.app) {
            tracing::warn!(%source, "could not emit webhookCaptured");
        }
    }

    fn stopped(&self, listener_id: &str, error: Option<&AppError>) {
        let event = WebhookStopped {
            listener_id: listener_id.to_string(),
            error: error.cloned(),
        };
        if let Err(source) = event.emit(&self.app) {
            tracing::warn!(%source, "could not emit webhookStopped");
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn list_webhooks(service: State<'_, WebhookService>) -> WebhookResult<Vec<WebhookListener>> {
    Ok(service.list())
}

#[tauri::command]
#[specta::specta]
pub async fn save_webhook(
    service: State<'_, WebhookService>,
    config: WebhookConfig,
) -> WebhookResult<WebhookListener> {
    service.save(config).await
}

#[tauri::command]
#[specta::specta]
pub async fn delete_webhook(service: State<'_, WebhookService>, id: String) -> WebhookResult<()> {
    service.delete(&id).await
}

#[tauri::command]
#[specta::specta]
pub async fn start_webhook(
    service: State<'_, WebhookService>,
    id: String,
) -> WebhookResult<WebhookListener> {
    service.start(&id).await
}

#[tauri::command]
#[specta::specta]
pub async fn stop_webhook(service: State<'_, WebhookService>, id: String) -> WebhookResult<()> {
    service.stop(&id).await
}

#[tauri::command]
#[specta::specta]
pub fn list_captures(
    service: State<'_, WebhookService>,
    id: String,
) -> WebhookResult<Vec<CaptureSummary>> {
    service.captures(&id)
}

#[tauri::command]
#[specta::specta]
pub fn read_capture(
    service: State<'_, WebhookService>,
    id: String,
    capture_id: String,
) -> WebhookResult<Capture> {
    service.read_capture(&id, &capture_id)
}

#[tauri::command]
#[specta::specta]
pub fn clear_captures(service: State<'_, WebhookService>, id: String) -> WebhookResult<()> {
    service.clear_captures(&id)
}

#[tauri::command]
#[specta::specta]
pub fn replay_capture(
    service: State<'_, WebhookService>,
    store: State<'_, StoreState>,
    id: String,
    capture_id: String,
) -> WebhookResult<ReplayResult> {
    let scratch = Scratch::new(store.lock_store().app_data())?;
    service.replay(&id, &capture_id, &scratch)
}
