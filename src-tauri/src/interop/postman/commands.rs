//! Postman IPC commands (plan 09 §7). Thin adapters: native dialogs, locks, events. The work
//! is in [`flow`] and [`export_flow`] so it runs in plain Rust tests.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_specta::Event;

use super::export_flow::{build_export, write_bundle};
use super::flow::{check_target, commit_into, commit_new, plan_paths};
use super::import::ImportPlan;
use super::stage::Stages;
use super::types::{
    ExportOptions, ImportOptions, ImportTarget, PostmanExportResult, PostmanImportResult,
    PostmanPreview,
};
use crate::error::AppError;
use crate::store::types::CollectionChanged;
use crate::store::StoreState;
use crate::vars::commands::VarsState;
use crate::vars::vault::SecretStore;

type PostmanResult<T> = Result<T, AppError>;

/// Managed by Tauri: staged previews and the vault (read only for "Include secret values").
pub struct PostmanState {
    stages: Mutex<Stages>,
    vault: Arc<dyn SecretStore>,
}

impl PostmanState {
    pub fn new(vault: Arc<dyn SecretStore>) -> Self {
        Self {
            stages: Mutex::new(Stages::default()),
            vault,
        }
    }

    fn stages(&self) -> MutexGuard<'_, Stages> {
        self.stages.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn logged<T>(result: PostmanResult<T>) -> PostmanResult<T> {
    if let Err(error) = &result {
        tracing::warn!(code = ?error.code, detail = ?error.detail, "{error}");
    }
    result
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> PostmanResult<T> + Send + 'static,
) -> PostmanResult<T> {
    let joined = tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|source| {
            AppError::internal("The operation failed.").with_detail(source.to_string())
        });
    logged(joined.and_then(|result| result))
}

fn to_path(picked: tauri_plugin_dialog::FilePath) -> PostmanResult<PathBuf> {
    picked.into_path().map_err(|source| {
        AppError::invalid_input("That file cannot be used.").with_detail(source.to_string())
    })
}

async fn pick_files(app: &AppHandle) -> PostmanResult<Option<Vec<PathBuf>>> {
    let app = app.clone();
    let picked = blocking(move || {
        Ok(app
            .dialog()
            .file()
            .set_title("Import Postman files")
            .add_filter("Postman JSON", &["json"])
            .blocking_pick_files())
    })
    .await?;
    picked
        .map(|files| files.into_iter().map(to_path).collect())
        .transpose()
}

async fn pick_folder(app: &AppHandle) -> PostmanResult<Option<PathBuf>> {
    let app = app.clone();
    let picked = blocking(move || Ok(app.dialog().file().blocking_pick_folder())).await?;
    picked.map(to_path).transpose()
}

#[tauri::command]
#[specta::specta]
pub async fn postman_import_pick(
    app: AppHandle,
    state: State<'_, PostmanState>,
) -> PostmanResult<Option<PostmanPreview>> {
    let Some(paths) = pick_files(&app).await? else {
        return Ok(None);
    };
    let (plan, report) = blocking(move || plan_paths(&paths)).await?;
    Ok(Some(state.stages().stage(plan, report, Instant::now())))
}

/// `None` when the folder pick of a new-collection import is cancelled; the token stays valid.
#[tauri::command]
#[specta::specta]
pub async fn postman_import_commit(
    app: AppHandle,
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    state: State<'_, PostmanState>,
    token: String,
    options: ImportOptions,
) -> PostmanResult<Option<PostmanImportResult>> {
    let staged = logged(state.stages().take(&token, Instant::now()))?;
    let into = match &options.target {
        ImportTarget::Collection { collection_id } => Some(collection_id.clone()),
        ImportTarget::NewCollection => None,
    };
    let outcome = commit(&app, &store, &vars, staged.plan().clone(), options).await;
    match outcome {
        Ok(Some(result)) => {
            emit_changed(&app, result.collection_id.clone().or(into));
            Ok(Some(result))
        }
        other => {
            state.stages().put_back(staged);
            other
        }
    }
}

async fn commit(
    app: &AppHandle,
    store: &State<'_, StoreState>,
    vars: &State<'_, VarsState>,
    plan: ImportPlan,
    options: ImportOptions,
) -> PostmanResult<Option<PostmanImportResult>> {
    logged(check_target(&plan, &options))?;
    let parent = match options.target {
        ImportTarget::NewCollection => match pick_folder(app).await? {
            Some(parent) => Some(parent),
            None => return Ok(None),
        },
        ImportTarget::Collection { .. } => None,
    };
    let shared = Arc::clone(&store.store);
    let service = vars.service.clone();
    let result = blocking(move || {
        let mut guard = shared.lock().unwrap_or_else(PoisonError::into_inner);
        match parent {
            Some(parent) => commit_new(&plan, &options, &parent, &mut guard, &service),
            None => commit_into(&plan, &options, &mut guard, &service),
        }
    })
    .await?;
    Ok(Some(result))
}

/// Tells the UI which collection changed (also for environment-only imports).
fn emit_changed(app: &AppHandle, collection_id: Option<String>) {
    let Some(collection_id) = collection_id else {
        return;
    };
    if let Err(error) = (CollectionChanged { collection_id }).emit(app) {
        tracing::warn!(%error, "could not emit collectionChanged");
    }
}

#[tauri::command]
#[specta::specta]
pub async fn postman_import_discard(
    state: State<'_, PostmanState>,
    token: String,
) -> PostmanResult<()> {
    state.stages().discard(&token);
    Ok(())
}

/// `None` when the folder pick is cancelled.
#[tauri::command]
#[specta::specta]
pub async fn postman_export(
    app: AppHandle,
    store: State<'_, StoreState>,
    state: State<'_, PostmanState>,
    options: ExportOptions,
) -> PostmanResult<Option<PostmanExportResult>> {
    let Some(dir) = pick_folder(&app).await? else {
        return Ok(None);
    };
    let shared = Arc::clone(&store.store);
    let vault = Arc::clone(&state.vault);
    let result = blocking(move || {
        let bundle = {
            let guard = shared.lock().unwrap_or_else(PoisonError::into_inner);
            build_export(&guard, vault.as_ref(), &options)?
        };
        write_bundle(&dir, bundle)
    })
    .await?;
    Ok(Some(result))
}
