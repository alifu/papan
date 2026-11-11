//! Native macOS menu bar.
//!
//! Tauri's default macOS menu binds ⌘W to "Close Window", which would close the whole
//! app before the webview sees the shortcut. Papan uses ⌘W to close a tab, so this menu
//! mirrors the default one but moves "Close Window" to ⇧⌘W.
//! Windows and Linux have no native menu by default, so nothing is installed there.

use tauri::menu::{AboutMetadata, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager, Runtime};

const CLOSE_WINDOW_ID: &str = "papan.close-window";
const CLOSE_WINDOW_ACCELERATOR: &str = "CmdOrCtrl+Shift+W";

/// Builds the macOS menu: App, File, Edit, View, Window.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let close_window = MenuItem::with_id(
        app,
        CLOSE_WINDOW_ID,
        "Close Window",
        true,
        Some(CLOSE_WINDOW_ACCELERATOR),
    )?;

    Menu::with_items(
        app,
        &[
            &app_submenu(app)?,
            &Submenu::with_items(app, "File", true, &[&close_window])?,
            &edit_submenu(app)?,
            &Submenu::with_items(
                app,
                "View",
                true,
                &[&PredefinedMenuItem::fullscreen(app, None)?],
            )?,
            &Submenu::with_items(
                app,
                "Window",
                true,
                &[
                    &PredefinedMenuItem::minimize(app, None)?,
                    &PredefinedMenuItem::maximize(app, None)?,
                ],
            )?,
        ],
    )
}

/// Handles clicks on the custom items of [`build`].
pub fn handle_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    if event.id() != CLOSE_WINDOW_ID {
        return;
    }
    let focused = app
        .webview_windows()
        .into_values()
        .find(|window| window.is_focused().unwrap_or(false));
    if let Some(window) = focused {
        if let Err(error) = window.close() {
            tracing::warn!(%error, "could not close window");
        }
    }
}

fn app_submenu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Submenu<R>> {
    let package = app.package_info();
    let about = AboutMetadata {
        name: Some(package.name.clone()),
        version: Some(package.version.to_string()),
        ..Default::default()
    };
    Submenu::with_items(
        app,
        package.name.clone(),
        true,
        &[
            &PredefinedMenuItem::about(app, None, Some(about))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )
}

/// Standard edit items: without them, copy/paste shortcuts do not work in the webview.
fn edit_submenu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Submenu<R>> {
    Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )
}
