//! Tauri commands 47–50 of plan 03b §8 (OAuth 2). Thin adapters over [`OAuth`]: the
//! draft is resolved with the scopes of `location`, as for a send. No command returns
//! a token; the webview gets [`OAuthStatus`].
//!
//! [`OAuth`]: super::OAuth

use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;
use url::Url;

use super::super::commands::logged;
use super::super::resolve::Resolved;
use super::super::variables::resolve_for;
use super::BrowserOpener;
use crate::engine::http::types::OAuthStatus;
use crate::engine::http::HttpEngine;
use crate::error::AppError;
use crate::store::types::{RequestContent, RequestLocation};
use crate::store::StoreState;
use crate::vars::commands::VarsState;

/// Opens the authorize URL in the system browser through the opener plugin.
struct SystemBrowser(AppHandle);

impl BrowserOpener for SystemBrowser {
    fn open(&self, url: &Url) -> Result<(), AppError> {
        // Checked again at the last step: only a web address reaches the OS.
        if !matches!(url.scheme(), "http" | "https") || !url.has_host() {
            return Err(AppError::invalid_input(
                "The authorization URL must be an http:// or https:// address.",
            ));
        }
        // No error text: it could quote the URL with its `state`.
        self.0
            .opener()
            .open_url(url.as_str(), None::<&str>)
            .map_err(|_| AppError::internal("Could not open the browser."))
    }
}

async fn resolved(
    store: &StoreState,
    vars: &VarsState,
    location: &RequestLocation,
    content: &RequestContent,
) -> Result<Resolved, AppError> {
    resolve_for(
        store,
        &vars.service,
        vars.dynamics.clone(),
        location,
        content,
    )
    .await
}

/// Command 47: the token state of the draft's resolved OAuth 2 configuration.
#[tauri::command]
#[specta::specta]
pub async fn oauth_status(
    engine: State<'_, HttpEngine>,
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    location: RequestLocation,
    content: RequestContent,
) -> Result<OAuthStatus, AppError> {
    let resolved = logged(resolved(&store, &vars, &location, &content).await)?;
    logged(engine.oauth().status(&location, &resolved).await)
}

/// Command 48: runs "Get new token" for the configuration the dialog showed (`config_key`).
// Three of the arguments are Tauri state; the IPC signature is fixed by the plan.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
#[specta::specta]
pub async fn oauth_authorize(
    app: AppHandle,
    engine: State<'_, HttpEngine>,
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    flow_id: String,
    location: RequestLocation,
    content: RequestContent,
    config_key: String,
) -> Result<OAuthStatus, AppError> {
    // Registered before resolving (which may wait on a keychain prompt), so an early
    // Cancel is not lost.
    let oauth = engine.oauth();
    let flow = logged(oauth.begin(&flow_id))?;
    let resolving = flow.unless_cancelled(resolved(&store, &vars, &location, &content));
    let resolved = logged(resolving.await)?;
    let browser = SystemBrowser(app);
    logged(
        oauth
            .authorize_pending(&flow, &location, &resolved, &config_key, &browser)
            .await,
    )
}

/// Command 49: cancels the pending flow `flow_id`.
#[tauri::command]
#[specta::specta]
pub fn oauth_cancel(engine: State<'_, HttpEngine>, flow_id: String) -> Result<(), AppError> {
    logged(engine.oauth().cancel(&flow_id))
}

/// Command 50: forgets the stored token of the draft's resolved configuration.
#[tauri::command]
#[specta::specta]
pub async fn oauth_clear(
    engine: State<'_, HttpEngine>,
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    location: RequestLocation,
    content: RequestContent,
) -> Result<OAuthStatus, AppError> {
    let resolved = logged(resolved(&store, &vars, &location, &content).await)?;
    logged(engine.oauth().clear(&location, &resolved).await)
}
