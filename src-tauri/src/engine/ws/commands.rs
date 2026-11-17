//! Tauri commands for WebSocket connections (plan 07 §7). Thin adapters over
//! [`WsEngine`]; the store lock is never held across a network await.

use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_specta::Event as _;

use super::handshake::connect_spec;
use super::messages::{finish_outgoing, outgoing_content};
use super::sink::BatchingSink;
use super::types::{WsMessage, WsOpened, WsOutgoing};
use super::WsEngine;
use crate::engine::http::variables::resolve_for;
use crate::engine::http::HttpEngine;
use crate::error::AppError;
use crate::store::request_model::validate_content;
use crate::store::types::{RequestContent, RequestKind, RequestLocation};
use crate::store::StoreState;
use crate::vars::commands::VarsState;

type WsResult<T> = Result<T, AppError>;

const CONTENT_LABEL: &str = "Request";

fn logged<T>(result: WsResult<T>) -> WsResult<T> {
    if let Err(error) = &result {
        tracing::warn!(code = ?error.code, detail = ?error.detail, "{error}");
    }
    result
}

/// Resolves the variables of `content` for `location`, opens the socket and streams its
/// events as `wsEvent` (messages batched every 50 ms).
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)] // one State per service the command needs
pub async fn ws_connect(
    app: AppHandle,
    engine: State<'_, WsEngine>,
    http: State<'_, HttpEngine>,
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    connection_id: String,
    location: RequestLocation,
    content: RequestContent,
) -> WsResult<WsOpened> {
    let opened = async {
        if content.kind != RequestKind::Ws {
            return Err(AppError::invalid_input("This is not a WebSocket request."));
        }
        validate_content(CONTENT_LABEL, &content)?;
        let resolved = resolve_for(
            &store,
            &vars.service,
            vars.dynamics.clone(),
            &location,
            &content,
        )
        .await?;
        let version = app.package_info().version.to_string();
        let spec = connect_spec(&http, &location, &resolved, &version).await?;
        let handle = app.clone();
        let sink = BatchingSink::new(&connection_id, move |event| {
            if let Err(source) = event.emit(&handle) {
                tracing::warn!("could not emit a WebSocket event: {source}");
            }
        });
        engine.open(&connection_id, spec, Arc::new(sink)).await
    };
    logged(opened.await)
}

/// Resolves the variables of `message` with the scopes of `location` and sends it.
#[tauri::command]
#[specta::specta]
pub async fn ws_send(
    engine: State<'_, WsEngine>,
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    connection_id: String,
    location: RequestLocation,
    message: WsOutgoing,
) -> WsResult<WsMessage> {
    let sent = async {
        let content = outgoing_content(&message);
        let resolved = resolve_for(
            &store,
            &vars.service,
            vars.dynamics.clone(),
            &location,
            &content,
        )
        .await?;
        let outbound = finish_outgoing(&message, &resolved)?;
        engine.send(&connection_id, outbound).await
    };
    logged(sent.await)
}

/// Closes the connection with the close handshake; also cancels a connect in progress.
#[tauri::command]
#[specta::specta]
pub async fn ws_disconnect(
    engine: State<'_, WsEngine>,
    connection_id: String,
    code: Option<u32>,
    reason: String,
) -> WsResult<()> {
    logged(engine.close(&connection_id, code, &reason).await)
}

#[tauri::command]
#[specta::specta]
pub fn ws_log(engine: State<'_, WsEngine>, connection_id: String) -> WsResult<Vec<WsMessage>> {
    logged(engine.log(&connection_id))
}

#[tauri::command]
#[specta::specta]
pub fn ws_clear_log(engine: State<'_, WsEngine>, connection_id: String) -> WsResult<()> {
    logged(engine.clear_log(&connection_id))
}
