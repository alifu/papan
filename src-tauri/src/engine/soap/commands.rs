//! Tauri commands for WSDL import and envelopes (plan 06 §8). Thin adapters over
//! [`WsdlService`]; the store lock is never held across a network await.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use url::Url;

use super::fetch::EngineFetch;
use super::service::{ImportRoot, WsdlService};
use super::{EnvelopeOptions, SoapTemplate, WsdlImport, WsdlSource};
use crate::engine::http::grants::grant_scope;
use crate::engine::http::resolve::{resolve_content, Resolved};
use crate::engine::http::HttpEngine;
use crate::error::AppError;
use crate::store::types::{RequestContent, RequestLocation, WsdlRef};
use crate::store::StoreState;
use crate::vars::commands::VarsState;

type SoapResult<T> = Result<T, AppError>;

const MAX_URL_CHARS: usize = 8192;
const EXTENSIONS: [&str; 3] = ["wsdl", "xsd", "xml"];

fn logged<T>(result: SoapResult<T>) -> SoapResult<T> {
    if let Err(error) = &result {
        tracing::warn!(code = ?error.code, detail = ?error.detail, "{error}");
    }
    result
}

fn service(app: &AppHandle) -> SoapResult<WsdlService> {
    let app_data = app.path().app_data_dir().map_err(|source| {
        AppError::internal("Could not locate the app data folder.").with_detail(source.to_string())
    })?;
    Ok(WsdlService::new(&app_data))
}

/// The WSDL URL resolved like a send (variables of `location`'s scopes), with the saved
/// request's auth, also resolved. Reads the store and the vault off the async runtime.
async fn resolve_root(
    store: &StoreState,
    vars: &VarsState,
    location: &RequestLocation,
    typed: &str,
) -> SoapResult<Resolved> {
    let store = Arc::clone(&store.store);
    let service = vars.service.clone();
    let dynamics = vars.dynamics.clone();
    let location = location.clone();
    let typed = typed.to_string();
    tokio::task::spawn_blocking(move || {
        let (auth, snapshot) = {
            let mut guard = store
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let auth = guard
                .read_request(&location)
                .map(|doc| doc.content.auth)
                .unwrap_or_default();
            (auth, service.snapshot(&guard, &location))
        };
        let content = RequestContent {
            auth,
            ..RequestContent::new("GET", &typed)
        };
        resolve_content(&content, &service.scopes(&snapshot), dynamics.as_ref())
    })
    .await
    .map_err(|source| {
        AppError::internal("Could not prepare the WSDL import.").with_detail(source.to_string())
    })
}

async fn import_url(
    engine: &HttpEngine,
    deps: (&StoreState, &VarsState, &WsdlService),
    import_id: &str,
    location: &RequestLocation,
    typed: &str,
) -> SoapResult<WsdlImport> {
    let (store, vars, wsdl) = deps;
    if typed.trim().is_empty() || typed.chars().count() > MAX_URL_CHARS {
        return Err(AppError::invalid_input("Enter the URL of a WSDL."));
    }
    let resolved = resolve_root(store, vars, location, typed).await?;
    if !resolved.unresolved.is_empty() {
        return Err(AppError::invalid_input(format!(
            "Unknown variables in the WSDL URL: {}.",
            resolved.unresolved.join(", ")
        )));
    }
    let url = Url::parse(resolved.content.url.trim())
        .map_err(|_| AppError::invalid_input("The WSDL URL is not valid."))?;
    let fetch = EngineFetch {
        engine,
        request_id: import_id.to_string(),
        location: location.clone(),
        auth: resolved.content.auth.clone(),
    };
    let root = ImportRoot::Url {
        typed: typed.to_string(),
        url,
    };
    let redact = |text: &str| resolved.redact(text);
    wsdl.import(&fetch, &grant_scope(location), root, &redact)
        .await
}

/// Imports a WSDL and its imports; `None` when the file dialog is cancelled.
/// Cancellable with `cancel_request(import_id)`.
#[tauri::command]
#[specta::specta]
pub async fn import_wsdl(
    app: AppHandle,
    engine: State<'_, HttpEngine>,
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    import_id: String,
    location: RequestLocation,
    source: WsdlSource,
) -> SoapResult<Option<WsdlImport>> {
    let wsdl = logged(service(&app))?;
    let url = match source {
        WsdlSource::Url { url } => url,
        WsdlSource::PickFile => {
            return logged(import_file(&app, &engine, &wsdl, &import_id, &location).await)
        }
    };
    let deps = (store.inner(), vars.inner(), &wsdl);
    logged(import_url(&engine, deps, &import_id, &location, &url).await).map(Some)
}

/// Picks a WSDL in the native dialog, grants it to `location` (M3 grants) and imports it.
async fn import_file(
    app: &AppHandle,
    engine: &HttpEngine,
    wsdl: &WsdlService,
    import_id: &str,
    location: &RequestLocation,
) -> SoapResult<Option<WsdlImport>> {
    let dialog_app = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        dialog_app
            .dialog()
            .file()
            .add_filter("WSDL", &EXTENSIONS)
            .blocking_pick_file()
    })
    .await
    .map_err(|source| {
        AppError::internal("The file dialog failed.").with_detail(source.to_string())
    })?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|source| {
        AppError::invalid_input("That file cannot be used.").with_detail(source.to_string())
    })?;
    let granted = engine.grant(location, &path)?;
    let fetch = EngineFetch {
        engine,
        request_id: import_id.to_string(),
        location: location.clone(),
        auth: crate::store::types::Auth::None,
    };
    let root = ImportRoot::File {
        path: PathBuf::from(granted.path),
    };
    let redact = |text: &str| text.to_string();
    wsdl.import(&fetch, &grant_scope(location), root, &redact)
        .await
        .map(Some)
}

/// The cached import behind a saved request's WSDL reference, without network.
#[tauri::command]
#[specta::specta]
pub async fn read_cached_wsdl(
    app: AppHandle,
    location: RequestLocation,
    wsdl: WsdlRef,
) -> SoapResult<Option<WsdlImport>> {
    logged(service(&app).and_then(|service| service.cached(&grant_scope(&location), &wsdl)))
}

/// Generates the envelope of one operation from the cached model.
#[tauri::command]
#[specta::specta]
pub async fn soap_envelope(
    app: AppHandle,
    wsdl_id: String,
    service: String,
    port: String,
    operation: String,
    options: EnvelopeOptions,
) -> SoapResult<SoapTemplate> {
    let wsdl = logged(self::service(&app))?;
    // Generation is bounded (steps, size) but CPU-bound: keep it off the async workers.
    let generated = tokio::task::spawn_blocking(move || {
        wsdl.envelope(&wsdl_id, &service, &port, &operation, options)
    })
    .await
    .map_err(|source| {
        AppError::internal("Could not generate the envelope.").with_detail(source.to_string())
    });
    logged(generated.and_then(|result| result))
}
