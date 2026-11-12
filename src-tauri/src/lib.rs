//! Papan core. Module layout follows the master plan (§3): plain-Rust logic per area,
//! with Tauri-specific code kept to `commands`, `ipc` and this file.

pub mod app_info;
pub mod commands;
pub mod engine;
pub mod error;
pub mod git;
pub mod interop;
pub mod ipc;
pub mod journey;
pub mod logging;
#[cfg(target_os = "macos")]
mod menu;
pub mod mock;
pub mod script;
pub mod store;
pub mod vars;
pub mod webhook;

use tauri::Manager;
use tracing_appender::non_blocking::WorkerGuard;

/// Keeps the background log writer alive for as long as the app runs.
struct LogGuard(#[allow(dead_code)] WorkerGuard);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let ipc_builder = ipc::builder();

    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.menu(menu::build).on_menu_event(menu::handle_event);

    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(ipc_builder.invoke_handler())
        .setup(move |app| {
            start_logging(app.handle());
            ipc_builder.mount_events(app);
            app.manage(store::StoreState::new(open_store(app.handle())?));
            let vault = open_vault(app.handle())?;
            app.manage(open_http_engine(
                app.handle(),
                std::sync::Arc::clone(&vault),
            )?);
            app.manage(vars::commands::VarsState::new(vault));
            #[cfg(debug_assertions)]
            export_bindings(&ipc_builder);
            tracing::info!(version = %app.package_info().version, "Papan started");
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Cookie saves are debounced; write any pending ones before the process ends.
            if let tauri::RunEvent::Exit = event {
                if let Some(engine) = app.try_state::<engine::http::HttpEngine>() {
                    tauri::async_runtime::block_on(engine.cookies().flush());
                }
            }
        });
}

/// Logging is not essential to using the app, so a failure here is reported on stderr
/// and the app carries on without a log file.
fn start_logging(app: &tauri::AppHandle) {
    let started = app
        .path()
        .app_log_dir()
        .map_err(|source| {
            error::AppError::internal("Could not locate the log directory.")
                .with_detail(source.to_string())
        })
        .and_then(|log_dir| logging::init(&log_dir));

    match started {
        Ok(guard) => {
            app.manage(LogGuard(guard));
        }
        Err(error) => eprintln!("logging disabled: {error} ({:?})", error.detail),
    }
}

/// The store is essential: without it the app cannot show anything, so startup fails.
fn open_store(app: &tauri::AppHandle) -> Result<store::Store, error::AppError> {
    let app_data = app.path().app_data_dir().map_err(|source| {
        error::AppError::internal("Could not locate the app data directory.")
            .with_detail(source.to_string())
    })?;
    store::Store::new(&app_data)
}

/// Sending requests is essential too; its cache dir is emptied at startup.
fn open_http_engine(
    app: &tauri::AppHandle,
    vault: std::sync::Arc<dyn vars::vault::SecretStore>,
) -> Result<engine::http::HttpEngine, error::AppError> {
    let locate = |found: Result<std::path::PathBuf, tauri::Error>| {
        found.map_err(|source| {
            error::AppError::internal("Could not locate the app directories.")
                .with_detail(source.to_string())
        })
    };
    let cache_dir = locate(app.path().app_cache_dir())?;
    let app_data = locate(app.path().app_data_dir())?;
    let version = app.package_info().version.to_string();
    Ok(
        engine::http::HttpEngine::new(&cache_dir, &app_data, &version)?
            .with_vault(&app_data, vault),
    )
}

/// Secrets: an encrypted vault whose key is in the OS keychain. Nothing is opened here
/// (the first secret access does it). Debug builds honour `PAPAN_SECRET_STORE=memory`
/// so `tauri dev` does not prompt on every rebuild. Variables and network settings share it.
fn open_vault(
    app: &tauri::AppHandle,
) -> Result<std::sync::Arc<dyn vars::vault::SecretStore>, error::AppError> {
    use std::sync::Arc;
    use vars::vault::{FileVault, OsKeys, SecretStore};

    #[cfg(debug_assertions)]
    if std::env::var("PAPAN_SECRET_STORE").as_deref() == Ok("memory") {
        tracing::info!("secrets are kept in memory (PAPAN_SECRET_STORE=memory)");
        let vault = vars::vault::MemoryVault::new(vars::types::SecretStorageMode::SessionOnly);
        return Ok(Arc::new(vault));
    }
    let app_data = app.path().app_data_dir().map_err(|source| {
        error::AppError::internal("Could not locate the app data directory.")
            .with_detail(source.to_string())
    })?;
    let vault: Arc<dyn SecretStore> = Arc::new(FileVault::new(
        app_data.join("secrets.vault"),
        OsKeys::new(),
    ));
    Ok(vault)
}

/// Keeps `src/ipc/bindings.ts` in step with the Rust commands during development.
#[cfg(debug_assertions)]
fn export_bindings(ipc_builder: &tauri_specta::Builder<tauri::Wry>) {
    match ipc::sync_bindings(ipc_builder, &ipc::bindings_path()) {
        Ok(ipc::BindingsSync::Written) => tracing::info!("IPC bindings regenerated"),
        Ok(ipc::BindingsSync::Unchanged) => {}
        Err(error) => tracing::warn!(detail = ?error.detail, "{error}"),
    }
}
