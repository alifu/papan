//! Tauri commands for environments, variables and secret storage (plan 04 §7). Thin
//! adapters over [`VarsService`]. Lock order: store, then vault; commands that may touch
//! the vault run in `spawn_blocking` (the first access can wait on a keychain prompt).

use std::sync::{Arc, PoisonError};

use tauri::State;

use super::dynamic::{Dynamics, SystemDynamics};
use super::service::{effective_variables as effective_of, preview};
use super::types::{
    EffectiveVariable, EnvironmentList, SecretStorageStatus, TemplatePreview, VarScope, Variable,
    VariableInput,
};
use super::vault::SecretStore;
use super::VarsService;
use crate::error::AppError;
use crate::store::types::RequestLocation;
use crate::store::{Store, StoreState};

type VarsResult<T> = Result<T, AppError>;

/// Managed by Tauri.
pub struct VarsState {
    pub service: VarsService,
    pub dynamics: Arc<dyn Dynamics>,
}

impl VarsState {
    pub fn new(vault: Arc<dyn SecretStore>) -> Self {
        Self {
            service: VarsService::new(vault),
            dynamics: Arc::new(SystemDynamics),
        }
    }
}

fn logged<T>(result: VarsResult<T>) -> VarsResult<T> {
    if let Err(error) = &result {
        tracing::warn!(code = ?error.code, detail = ?error.detail, "{error}");
    }
    result
}

/// Runs `work` with the store locked, off the async runtime.
async fn blocking<T: Send + 'static>(
    store: &StoreState,
    vars: &VarsState,
    work: impl FnOnce(&mut Store, &VarsService) -> VarsResult<T> + Send + 'static,
) -> VarsResult<T> {
    let store = Arc::clone(&store.store);
    let service = vars.service.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let mut guard = store.lock().unwrap_or_else(PoisonError::into_inner);
        work(&mut guard, &service)
    })
    .await
    .map_err(|source| AppError::internal("The operation failed.").with_detail(source.to_string()));
    logged(joined.and_then(|result| result))
}

/// Resolver layers for `location`; the store lock is released before the vault is read.
async fn scopes_for(
    store: &StoreState,
    vars: &VarsState,
    location: RequestLocation,
) -> VarsResult<super::scopes::Scopes> {
    let store = Arc::clone(&store.store);
    let service = vars.service.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let snapshot = {
            let guard = store.lock().unwrap_or_else(PoisonError::into_inner);
            service.snapshot(&guard, &location)
        };
        service.scopes(&snapshot)
    })
    .await
    .map_err(|source| AppError::internal("The operation failed.").with_detail(source.to_string()))
}

#[tauri::command]
#[specta::specta]
pub fn list_environments(
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    collection_id: String,
) -> VarsResult<EnvironmentList> {
    logged(
        vars.service
            .list_environments(&store.lock_store(), &collection_id),
    )
}

#[tauri::command]
#[specta::specta]
pub fn create_environment(
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    collection_id: String,
    name: String,
) -> VarsResult<EnvironmentList> {
    logged(
        vars.service
            .create_environment(&mut store.lock_store(), &collection_id, &name),
    )
}

#[tauri::command]
#[specta::specta]
pub fn rename_environment(
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    collection_id: String,
    env: String,
    name: String,
) -> VarsResult<EnvironmentList> {
    logged(
        vars.service
            .rename_environment(&mut store.lock_store(), &collection_id, &env, &name),
    )
}

/// Also deletes the environment's local values and secrets.
#[tauri::command]
#[specta::specta]
pub async fn delete_environment(
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    collection_id: String,
    env: String,
) -> VarsResult<EnvironmentList> {
    blocking(&store, &vars, move |store, service| {
        service.delete_environment(store, &collection_id, &env)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub fn select_environment(
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    collection_id: String,
    env: Option<String>,
) -> VarsResult<()> {
    logged(
        vars.service
            .select_environment(&store.lock_store(), &collection_id, env.as_deref()),
    )
}

/// Variables of one scope, sorted by name.
#[tauri::command]
#[specta::specta]
pub async fn read_variables(
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    scope: VarScope,
) -> VarsResult<Vec<Variable>> {
    blocking(&store, &vars, move |store, service| {
        service.read_variables(store, &scope)
    })
    .await
}

/// Replaces the whole scope: rows not listed are deleted.
#[tauri::command]
#[specta::specta]
pub async fn write_variables(
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    scope: VarScope,
    variables: Vec<VariableInput>,
) -> VarsResult<Vec<Variable>> {
    blocking(&store, &vars, move |store, service| {
        service.write_variables(store, &scope, &variables)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn effective_variables(
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    location: RequestLocation,
) -> VarsResult<Vec<EffectiveVariable>> {
    let scopes = logged(scopes_for(&store, &vars, location).await)?;
    Ok(effective_of(&scopes))
}

#[tauri::command]
#[specta::specta]
pub async fn preview_template(
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    location: RequestLocation,
    text: String,
) -> VarsResult<TemplatePreview> {
    let scopes = logged(scopes_for(&store, &vars, location).await)?;
    logged(preview(&scopes, &text))
}

// Opens the vault first, so the mode reported is the real one.
#[tauri::command]
#[specta::specta]
pub async fn secret_storage_status(vars: State<'_, VarsState>) -> VarsResult<SecretStorageStatus> {
    let service = vars.service.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || service.probe_status()).await;
    logged(joined.map_err(|source| {
        AppError::internal("The operation failed.").with_detail(source.to_string())
    }))
}

#[tauri::command]
#[specta::specta]
pub async fn reset_secret_storage(vars: State<'_, VarsState>) -> VarsResult<SecretStorageStatus> {
    let service = vars.service.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || service.reset()).await;
    logged(
        joined
            .map_err(|source| {
                AppError::internal("The operation failed.").with_detail(source.to_string())
            })
            .and_then(|result| result),
    )
}
