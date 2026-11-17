//! App-level Tauri commands. Thin adapters only: logic lives in plain-Rust modules.

use crate::app_info::AppInfo;

/// Name and version of the running app, from the bundle's package info.
#[tauri::command]
#[specta::specta]
pub fn app_info(app: tauri::AppHandle) -> AppInfo {
    let package = app.package_info();
    AppInfo::new(package.name.clone(), &package.version)
}
