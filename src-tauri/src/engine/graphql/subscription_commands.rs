//! Tauri commands for GraphQL subscriptions over `graphql-transport-ws` (plan 06b §6).
//! Thin adapters over [`Subscriptions`]; the store lock is never held across an await.

use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_specta::Event as _;

use super::subscription_service::Subscriptions;
use crate::engine::http::variables::resolve_for;
use crate::engine::http::HttpEngine;
use crate::engine::ws::sink::BatchingSink;
use crate::engine::ws::types::WsOpened;
use crate::engine::ws::WsEngine;
use crate::error::AppError;
use crate::store::request_model::validate_content;
use crate::store::types::{RequestContent, RequestKind, RequestLocation};
use crate::store::StoreState;
use crate::vars::commands::VarsState;

const CONTENT_LABEL: &str = "Request";

fn logged<T>(result: Result<T, AppError>) -> Result<T, AppError> {
    if let Err(error) = &result {
        tracing::warn!(code = ?error.code, detail = ?error.detail, "{error}");
    }
    result
}

/// Resolves `content`, opens a `graphql-transport-ws` socket and starts the
/// subscription; resolves after `connection_ack`. Frames stream as `wsEvent`.
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)] // one State per service the command needs
pub async fn graphql_subscribe(
    app: AppHandle,
    engine: State<'_, WsEngine>,
    http: State<'_, HttpEngine>,
    subscriptions: State<'_, Subscriptions>,
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    connection_id: String,
    location: RequestLocation,
    content: RequestContent,
) -> Result<WsOpened, AppError> {
    let opened = async {
        if content.kind != RequestKind::Graphql {
            return Err(AppError::invalid_input("This is not a GraphQL request."));
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
        let handle = app.clone();
        let sink = BatchingSink::new(&connection_id, move |event| {
            if let Err(source) = event.emit(&handle) {
                tracing::warn!("could not emit a WebSocket event: {source}");
            }
        });
        subscriptions
            .subscribe(
                &engine,
                &http,
                &connection_id,
                &location,
                &content,
                &resolved,
                Arc::new(sink),
                &version,
            )
            .await
    };
    logged(opened.await)
}

/// Sends `complete` and closes the socket; also cancels a subscription that is starting.
#[tauri::command]
#[specta::specta]
pub async fn graphql_unsubscribe(
    engine: State<'_, WsEngine>,
    subscriptions: State<'_, Subscriptions>,
    connection_id: String,
) -> Result<(), AppError> {
    logged(subscriptions.unsubscribe(&engine, &connection_id).await)
}
