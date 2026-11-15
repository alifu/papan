//! Tauri commands for gRPC (plan 08 §6). Thin adapters over [`GrpcDeps`] and
//! [`super::GrpcEngine`]; the store lock is never held across a network await.

use std::sync::Arc;

use tauri::{AppHandle, Manager as _, State};
use tauri_plugin_dialog::DialogExt as _;
use tauri_specta::Event as _;

use super::service::{GrpcDeps, GrpcState};
use super::sink::{GrpcBatchingSink, GrpcInbound};
use super::types::{GrpcSchema, GrpcStarted, ProtoImport};
use crate::engine::http::HttpEngine;
use crate::engine::ws::types::WsMessage;
use crate::error::AppError;
use crate::store::types::{RequestContent, RequestLocation};
use crate::store::StoreState;
use crate::vars::commands::VarsState;

type GrpcResult<T> = Result<T, AppError>;

const PROTO_EXTENSIONS: [&str; 1] = ["proto"];

fn logged<T>(result: GrpcResult<T>) -> GrpcResult<T> {
    if let Err(error) = &result {
        tracing::warn!(code = ?error.code, detail = ?error.detail, "{error}");
    }
    result
}

/// The gRPC state of the app, created on first use (its cache lives in app data).
fn state(app: &AppHandle) -> GrpcResult<State<'_, GrpcState>> {
    if app.try_state::<GrpcState>().is_none() {
        let app_data = app.path().app_data_dir().map_err(|source| {
            AppError::internal("Could not locate the app data directory.")
                .with_detail(source.to_string())
        })?;
        // `manage` keeps the first value when two commands race here.
        app.manage(GrpcState::new(&app_data));
    }
    Ok(app.state::<GrpcState>())
}

fn deps<'a>(
    http: &'a HttpEngine,
    store: &'a StoreState,
    vars: &'a VarsState,
    state: &'a GrpcState,
    version: &'a str,
) -> GrpcDeps<'a> {
    GrpcDeps {
        http,
        store,
        vars: &vars.service,
        dynamics: vars.dynamics.clone(),
        state,
        version,
    }
}

/// The services and methods of the request: its proto files compiled, or reflection
/// from the cache. Without `allow_network` a reflection request with no cache entry
/// answers `None` and nothing is sent; with it the server is asked.
#[tauri::command]
#[specta::specta]
pub async fn grpc_schema(
    app: AppHandle,
    http: State<'_, HttpEngine>,
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    location: RequestLocation,
    content: RequestContent,
    allow_network: bool,
) -> GrpcResult<Option<GrpcSchema>> {
    let state = logged(state(&app))?;
    let version = app.package_info().version.to_string();
    let deps = deps(&http, &store, &vars, &state, &version);
    logged(deps.schema(&location, &content, allow_network).await)
}

/// Server reflection for the request's server; `refresh` fetches again and rewrites the
/// cache.
#[tauri::command]
#[specta::specta]
pub async fn grpc_reflect(
    app: AppHandle,
    http: State<'_, HttpEngine>,
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    location: RequestLocation,
    content: RequestContent,
    refresh: bool,
) -> GrpcResult<GrpcSchema> {
    let state = logged(state(&app))?;
    let version = app.package_info().version.to_string();
    let deps = deps(&http, &store, &vars, &state, &version);
    logged(deps.reflect(&location, &content, refresh).await)
}

/// Picks a `.proto` file in the native dialog and copies it, with what it imports, into
/// the protos folder of `location`. Nothing is added when the dialog is cancelled.
#[tauri::command]
#[specta::specta]
pub async fn grpc_import_proto(
    app: AppHandle,
    http: State<'_, HttpEngine>,
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    location: RequestLocation,
) -> GrpcResult<ProtoImport> {
    let state = logged(state(&app))?;
    let dialog_app = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        dialog_app
            .dialog()
            .file()
            .add_filter("Protocol Buffers", &PROTO_EXTENSIONS)
            .blocking_pick_file()
    })
    .await
    .map_err(|source| {
        AppError::internal("The file dialog failed.").with_detail(source.to_string())
    });
    let Some(picked) = logged(picked)? else {
        return Ok(ProtoImport {
            added: Vec::new(),
            warnings: Vec::new(),
        });
    };
    let path = logged(picked.into_path().map_err(|source| {
        AppError::invalid_input("That file cannot be used.").with_detail(source.to_string())
    }))?;
    let version = app.package_info().version.to_string();
    let deps = deps(&http, &store, &vars, &state, &version);
    logged(deps.import(&location, path).await)
}

/// Starts the call of the request and streams its events as `grpcEvent`. A call still
/// running under `call_id` is cancelled first. Cancellable with `grpc_cancel`.
#[tauri::command]
#[specta::specta]
pub async fn grpc_start(
    app: AppHandle,
    http: State<'_, HttpEngine>,
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    call_id: String,
    location: RequestLocation,
    content: RequestContent,
) -> GrpcResult<GrpcStarted> {
    let state = logged(state(&app))?;
    let version = app.package_info().version.to_string();
    let deps = deps(&http, &store, &vars, &state, &version);
    let (handle, id) = (app.clone(), call_id.clone());
    let sink_for = move |attempt: u32| -> Arc<dyn GrpcInbound> {
        Arc::new(GrpcBatchingSink::new(&id, attempt, move |event| {
            if let Err(source) = event.emit(&handle) {
                tracing::warn!("could not emit a gRPC event: {source}");
            }
        }))
    };
    let prepare = deps.prepare(&location, &content);
    logged(state.engine.start(&call_id, prepare, sink_for).await)
}

/// Resolves the variables of `message` with the scopes of `location` and sends it on a
/// client or bidi stream.
#[tauri::command]
#[specta::specta]
pub async fn grpc_send(
    app: AppHandle,
    http: State<'_, HttpEngine>,
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    call_id: String,
    location: RequestLocation,
    message: String,
) -> GrpcResult<WsMessage> {
    let state = logged(state(&app))?;
    let version = app.package_info().version.to_string();
    let deps = deps(&http, &store, &vars, &state, &version);
    let sent = async {
        let (json, secrets) = deps.resolve_message(&location, &message).await?;
        state.engine.send(&call_id, &json, &secrets).await
    };
    logged(sent.await)
}

/// Half-closes the request stream of a client or bidi call.
#[tauri::command]
#[specta::specta]
pub async fn grpc_end(app: AppHandle, call_id: String) -> GrpcResult<()> {
    logged(state(&app).and_then(|state| state.engine.end(&call_id)))
}

/// Cancels the call or a start in progress. A finished call is not an error.
#[tauri::command]
#[specta::specta]
pub async fn grpc_cancel(app: AppHandle, call_id: String) -> GrpcResult<()> {
    logged(state(&app)).map(|state| state.engine.cancel(&call_id))
}

#[tauri::command]
#[specta::specta]
pub async fn grpc_log(app: AppHandle, call_id: String) -> GrpcResult<Vec<WsMessage>> {
    logged(state(&app)).map(|state| state.engine.log(&call_id))
}

#[tauri::command]
#[specta::specta]
pub async fn grpc_clear_log(app: AppHandle, call_id: String) -> GrpcResult<()> {
    logged(state(&app)).map(|state| state.engine.clear_log(&call_id))
}
