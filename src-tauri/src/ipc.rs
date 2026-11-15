//! The typed IPC contract: every command and shared type is registered here, and the
//! TypeScript bindings in `src/ipc/bindings.ts` are generated from it.
//!
//! Bindings are rewritten automatically when a debug build of the app starts. To
//! regenerate without launching the app: `UPDATE_BINDINGS=1 cargo test bindings`.
//! Plain `cargo test` fails if the committed file is stale.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use specta_typescript::Typescript;
use tauri_specta::{collect_commands, collect_events, Builder};

use crate::commands;
use crate::engine::graphql::commands as graphql;
use crate::engine::graphql::subscription_commands as graphql_sub;
use crate::engine::http::commands as http;
use crate::engine::http::net_commands as net;
use crate::engine::http::oauth::commands as oauth;
use crate::engine::soap::commands as soap;
use crate::engine::ws::commands as ws;
use crate::engine::ws::types::WsEvent;
use crate::error::AppError;
use crate::store::commands as store;
use crate::store::types::CollectionChanged;
use crate::vars::commands as vars;
use crate::webhook::commands as webhook;
use crate::webhook::types::{WebhookCaptured, WebhookStopped};

/// Result of [`sync_bindings`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingsSync {
    Unchanged,
    Written,
}

/// Builds the command/type registry. Add new commands and IPC types here.
///
/// Serde phases are disabled so each Rust type maps to exactly one TypeScript type
/// (otherwise `skip_serializing_if` splits a type into `_Serialize`/`_Deserialize`).
pub fn builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::app_info,
            store::list_collections,
            store::create_collection,
            store::open_collection,
            store::close_collection,
            store::read_tree,
            store::create_folder,
            store::create_request,
            store::rename_node,
            store::delete_node,
            store::move_node,
            store::read_request,
            store::write_request,
            store::read_scripts,
            store::write_scripts,
            store::list_scratch,
            store::create_scratch,
            store::rename_scratch,
            store::delete_scratch,
            store::move_scratch_to_collection,
            http::send_request,
            http::cancel_request,
            http::read_response_body,
            http::save_response_body,
            http::pick_body_file,
            http::clear_cookies,
            store::list_history,
            store::clear_history,
            store::restore_history_entry,
            vars::list_environments,
            vars::create_environment,
            vars::rename_environment,
            vars::delete_environment,
            vars::select_environment,
            vars::read_variables,
            vars::write_variables,
            vars::effective_variables,
            vars::preview_template,
            vars::secret_storage_status,
            vars::reset_secret_storage,
            net::read_network_settings,
            net::read_proxy_mode,
            net::write_proxy_settings,
            net::add_client_certificate,
            net::remove_client_certificate,
            net::list_cookies,
            net::set_cookie,
            net::delete_cookies,
            net::export_curl,
            net::import_curl,
            oauth::oauth_status,
            oauth::oauth_authorize,
            oauth::oauth_cancel,
            oauth::oauth_clear,
            graphql::introspect_graphql,
            graphql::read_cached_schema,
            graphql::load_schema_file,
            graphql::clear_cached_schema,
            graphql_sub::graphql_subscribe,
            graphql_sub::graphql_unsubscribe,
            soap::import_wsdl,
            soap::read_cached_wsdl,
            soap::soap_envelope,
            ws::ws_connect,
            ws::ws_send,
            ws::ws_disconnect,
            ws::ws_log,
            ws::ws_clear_log,
            webhook::list_webhooks,
            webhook::save_webhook,
            webhook::delete_webhook,
            webhook::start_webhook,
            webhook::stop_webhook,
            webhook::list_captures,
            webhook::read_capture,
            webhook::clear_captures,
            webhook::replay_capture,
        ])
        .events(collect_events![
            CollectionChanged,
            WsEvent,
            WebhookCaptured,
            WebhookStopped
        ])
        .typ::<AppError>()
        .disable_serde_phases()
}

/// Location of the committed bindings file, relative to this crate at compile time.
pub fn bindings_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/ipc/bindings.ts")
}

/// Renders the TypeScript bindings to a string (line endings normalised to `\n`).
pub fn render_bindings(builder: &Builder<tauri::Wry>) -> Result<String, AppError> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let scratch = std::env::temp_dir().join(format!(
        "papan-bindings-{}-{}.ts",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));

    let exported = builder
        .export(Typescript::default(), &scratch)
        .map_err(|source| {
            AppError::internal("Could not generate IPC bindings.").with_detail(source.to_string())
        })
        .and_then(|()| Ok(std::fs::read_to_string(&scratch)?));
    // Best effort: a leftover scratch file in the temp dir is harmless.
    let _ = std::fs::remove_file(&scratch);

    exported.map(|text| normalise_newlines(&text))
}

/// Writes the bindings to `path` only when they differ from what is already there,
/// so an unchanged contract does not trigger a frontend reload.
pub fn sync_bindings(builder: &Builder<tauri::Wry>, path: &Path) -> Result<BindingsSync, AppError> {
    let rendered = render_bindings(builder)?;
    let current = std::fs::read_to_string(path).ok();

    if current.as_deref().map(normalise_newlines).as_deref() == Some(rendered.as_str()) {
        return Ok(BindingsSync::Unchanged);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, rendered)?;
    Ok(BindingsSync::Written)
}

fn normalise_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const UPDATE_ENV: &str = "UPDATE_BINDINGS";

    #[test]
    fn bindings_file_is_up_to_date() {
        let path = bindings_path();
        if std::env::var_os(UPDATE_ENV).is_some() {
            sync_bindings(&builder(), &path).unwrap();
        }

        let rendered = render_bindings(&builder()).unwrap();
        let committed = std::fs::read_to_string(&path).map(|text| normalise_newlines(&text));

        assert!(
            committed.as_deref().ok() == Some(rendered.as_str()),
            "{} is stale or missing. Regenerate it with \
             `UPDATE_BINDINGS=1 cargo test bindings` and commit the result.",
            path.display()
        );
    }

    #[test]
    fn bindings_expose_app_info_command_and_error_type() {
        let rendered = render_bindings(&builder()).unwrap();

        assert!(rendered.contains("appInfo"));
        assert!(rendered.contains("AppError"));
        assert!(rendered.contains("\"invalid_input\""));
        for code in ["\"network\"", "\"timeout\"", "\"cancelled\""] {
            assert!(rendered.contains(code), "bindings are missing {code}");
        }
    }

    #[test]
    fn bindings_expose_store_commands_and_collection_changed_event() {
        let rendered = render_bindings(&builder()).unwrap();
        let expected = [
            "listCollections",
            "createCollection",
            "openCollection",
            "closeCollection",
            "readTree",
            "createFolder",
            "createRequest",
            "renameNode",
            "deleteNode",
            "moveNode",
            "readRequest",
            "writeRequest",
            "listScratch",
            "createScratch",
            "renameScratch",
            "deleteScratch",
            "moveScratchToCollection",
            "collectionChanged",
            "sendRequest",
            "cancelRequest",
            "readResponseBody",
            "saveResponseBody",
            "pickBodyFile",
            "clearCookies",
            "listHistory",
            "clearHistory",
            "restoreHistoryEntry",
            "listEnvironments",
            "createEnvironment",
            "renameEnvironment",
            "deleteEnvironment",
            "selectEnvironment",
            "readVariables",
            "writeVariables",
            "effectiveVariables",
            "previewTemplate",
            "secretStorageStatus",
            "resetSecretStorage",
            "readNetworkSettings",
            "readProxyMode",
            "writeProxySettings",
            "addClientCertificate",
            "removeClientCertificate",
            "listCookies",
            "setCookie",
            "deleteCookies",
            "exportCurl",
            "importCurl",
            "oauthStatus",
            "oauthAuthorize",
            "oauthCancel",
            "oauthClear",
            "introspectGraphql",
            "readCachedSchema",
            "loadSchemaFile",
            "clearCachedSchema",
            "importWsdl",
            "readCachedWsdl",
            "soapEnvelope",
            "wsConnect",
            "wsSend",
            "wsDisconnect",
            "wsLog",
            "wsClearLog",
            "listWebhooks",
            "saveWebhook",
            "deleteWebhook",
            "startWebhook",
            "stopWebhook",
            "listCaptures",
            "readCapture",
            "clearCaptures",
            "replayCapture",
            "wsEvent",
            "webhookCaptured",
            "webhookStopped",
        ];

        let missing: Vec<&str> = expected
            .into_iter()
            .filter(|name| {
                !rendered.contains(&format!("{name}(")) && !rendered.contains(&format!("{name}:"))
            })
            .collect();

        assert!(missing.is_empty(), "bindings are missing {missing:?}");
        assert!(rendered.contains("export type OAuthStatus"));
    }

    #[test]
    fn sync_writes_once_then_reports_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ipc").join("bindings.ts");

        let first = sync_bindings(&builder(), &path).unwrap();
        let second = sync_bindings(&builder(), &path).unwrap();

        assert_eq!(first, BindingsSync::Written);
        assert_eq!(second, BindingsSync::Unchanged);
    }
}
