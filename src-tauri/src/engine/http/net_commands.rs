//! Tauri commands 38–46 of plan 03b §8: network settings, cookie manager and cURL.
//!
//! 38–41 (network settings, O4) and 42–44 (cookie manager, O5) are real.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use super::commands::{dialog, logged, picked_path};
use super::identity::{self, MAX_BUNDLE_BYTES};
use super::network::Network;
use super::types::{
    CookieFilter, CookieInput, CurlImport, NetworkSettings, ProxyInput, ProxyMode, StoredCookie,
};
use super::HttpEngine;
use crate::error::AppError;
use crate::store::types::{RequestContent, RequestLocation};
use crate::vars::commands::VarsState;

type NetResult<T> = Result<T, AppError>;

/// Runs vault-touching network work off the async runtime (it may wait on a prompt).
async fn blocking<T: Send + 'static>(
    network: Arc<Network>,
    work: impl FnOnce(&Network) -> NetResult<T> + Send + 'static,
) -> NetResult<T> {
    let result = tauri::async_runtime::spawn_blocking(move || work(&network))
        .await
        .map_err(|source| {
            AppError::internal("The operation failed.").with_detail(source.to_string())
        })
        .and_then(|result| result);
    logged(result)
}

/// Proxy and client certificates as stored on this machine, with the vault's mode.
#[tauri::command]
#[specta::specta]
pub async fn read_network_settings(engine: State<'_, HttpEngine>) -> NetResult<NetworkSettings> {
    blocking(engine.network(), Network::settings).await
}

/// Command 51: the proxy mode only, read from `network.json` without opening the vault,
/// so the startup "Proxy" badge never triggers a keychain prompt.
#[tauri::command]
#[specta::specta]
pub async fn read_proxy_mode(engine: State<'_, HttpEngine>) -> NetResult<ProxyMode> {
    Ok(engine.network().proxy_mode())
}

/// Replaces the proxy settings; the password follows `SecretEdit`.
#[tauri::command]
#[specta::specta]
pub async fn write_proxy_settings(
    engine: State<'_, HttpEngine>,
    proxy: ProxyInput,
) -> NetResult<NetworkSettings> {
    blocking(engine.network(), move |network| network.write_proxy(&proxy)).await
}

/// Whether the first picked file already holds a private key (then no key dialog).
fn has_private_key(path: &Path) -> bool {
    use std::io::Read;
    let mut head = Vec::new();
    let read = std::fs::File::open(path)
        .and_then(|file| file.take(MAX_BUNDLE_BYTES as u64).read_to_end(&mut head));
    read.is_ok() && String::from_utf8_lossy(&head).contains("PRIVATE KEY-----")
}

async fn pick(app: &AppHandle, title: &'static str) -> NetResult<Option<PathBuf>> {
    let app = app.clone();
    let picked = dialog(move || app.dialog().file().set_title(title).blocking_pick_file()).await?;
    picked.map(picked_path).transpose()
}

/// Imports a PEM certificate and key for `host` (and `port`) through native dialogs;
/// `None` when a dialog is cancelled.
#[tauri::command]
#[specta::specta]
pub async fn add_client_certificate(
    app: AppHandle,
    engine: State<'_, HttpEngine>,
    host: String,
    port: Option<u16>,
) -> NetResult<Option<NetworkSettings>> {
    logged(identity::normalize_host(&host))?;
    let Some(cert_file) = pick(&app, "Choose the client certificate (PEM)").await? else {
        return Ok(None);
    };
    let key_file = if has_private_key(&cert_file) {
        None
    } else {
        match pick(&app, "Choose the private key (PEM)").await? {
            Some(key_file) => Some(key_file),
            None => return Ok(None),
        }
    };
    let added = blocking(engine.network(), move |network| {
        network.add_certificate(&host, port, &cert_file, key_file.as_deref())
    })
    .await?;
    Ok(Some(added))
}

#[tauri::command]
#[specta::specta]
pub async fn remove_client_certificate(
    engine: State<'_, HttpEngine>,
    id: String,
) -> NetResult<NetworkSettings> {
    blocking(engine.network(), move |network| {
        network.remove_certificate(&id)
    })
    .await
}

/// The unexpired cookies of the jar `location` uses, sorted by domain, path, name.
#[tauri::command]
#[specta::specta]
pub async fn list_cookies(
    engine: State<'_, HttpEngine>,
    location: RequestLocation,
) -> NetResult<Vec<StoredCookie>> {
    logged(engine.cookies().list(&location).await)
}

/// Adds or replaces one cookie; returns the jar's new list.
#[tauri::command]
#[specta::specta]
pub async fn set_cookie(
    engine: State<'_, HttpEngine>,
    location: RequestLocation,
    cookie: CookieInput,
) -> NetResult<Vec<StoredCookie>> {
    logged(engine.cookies().set(&location, &cookie).await)
}

/// Deletes the cookies `filter` names; returns the jar's new list.
#[tauri::command]
#[specta::specta]
pub async fn delete_cookies(
    engine: State<'_, HttpEngine>,
    location: RequestLocation,
    filter: CookieFilter,
) -> NetResult<Vec<StoredCookie>> {
    logged(engine.cookies().delete(&location, &filter).await)
}

/// The draft as a POSIX cURL command; secret variables stay as `{{name}}`.
#[tauri::command]
#[specta::specta]
pub async fn export_curl(
    store: State<'_, crate::store::StoreState>,
    vars: State<'_, VarsState>,
    location: RequestLocation,
    content: RequestContent,
) -> NetResult<String> {
    let store = std::sync::Arc::clone(&store.store);
    let service = vars.service.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let guard = store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        crate::interop::curl::export_masked(&service, &guard, &location, &content)
    })
    .await
    .map_err(|source| AppError::internal("The operation failed.").with_detail(source.to_string()));
    let result = joined.and_then(|exported| exported);
    if let Err(error) = &result {
        tracing::warn!(code = ?error.code, "{error}");
    }
    result
}

/// Parses a cURL command without executing or reading anything.
#[tauri::command]
#[specta::specta]
pub fn import_curl(text: String) -> NetResult<CurlImport> {
    crate::interop::curl::import(&text)
}
