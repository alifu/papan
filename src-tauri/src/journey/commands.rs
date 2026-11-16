//! Tauri commands for journeys (plan 13 §6). Thin adapters over `disk` and the runner.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use tauri::{AppHandle, State};
use tauri_specta::Event;

use super::disk;
use super::host::EngineHost;
use super::model::Journey;
use super::runner::run;
use super::types::{JourneyRunEvent, JourneySummary, RunSummary};
use crate::engine::http::script_map::secrets_of;
use crate::engine::http::scripted::SendDeps;
use crate::engine::http::HttpEngine;
use crate::error::AppError;
use crate::store::types::RequestLocation;
use crate::store::StoreState;
use crate::vars::commands::VarsState;

type JourneyResult<T> = Result<T, AppError>;

/// What `cancel_journey` needs of a running journey: its stop flag and the id of the request
/// or script it is in.
#[derive(Clone, Default)]
pub struct RunHandle {
    pub cancel: Arc<AtomicBool>,
    pub current: Arc<Mutex<Option<String>>>,
}

/// The journeys that are running, by collection and file.
#[derive(Default)]
pub struct JourneyRuns {
    running: Mutex<HashMap<String, RunHandle>>,
}

impl JourneyRuns {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, RunHandle>> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn begin(&self, key: &str) -> Option<RunHandle> {
        let mut running = self.lock();
        if running.contains_key(key) {
            return None;
        }
        let handle = RunHandle::default();
        running.insert(key.to_string(), handle.clone());
        Some(handle)
    }

    fn end(&self, key: &str) {
        self.lock().remove(key);
    }

    fn handle(&self, key: &str) -> Option<RunHandle> {
        self.lock().get(key).cloned()
    }
}

fn key(collection_id: &str, file: &str) -> String {
    format!("{collection_id}/{file}")
}

fn root_of(state: &StoreState, collection_id: &str) -> JourneyResult<PathBuf> {
    state.lock_store().collection_root(collection_id)
}

fn logged<T>(result: JourneyResult<T>) -> JourneyResult<T> {
    if let Err(error) = &result {
        tracing::warn!(code = ?error.code, detail = ?error.detail, "{error}");
    }
    result
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> JourneyResult<T> + Send + 'static,
) -> JourneyResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|source| {
            AppError::internal("The journey task failed.").with_detail(source.to_string())
        })?
}

#[tauri::command]
#[specta::specta]
pub async fn list_journeys(
    state: State<'_, StoreState>,
    collection_id: String,
) -> JourneyResult<Vec<JourneySummary>> {
    let root = root_of(&state, &collection_id)?;
    logged(blocking(move || Ok(disk::list(&root))).await)
}

#[tauri::command]
#[specta::specta]
pub async fn read_journey(
    state: State<'_, StoreState>,
    collection_id: String,
    file: String,
) -> JourneyResult<Journey> {
    let root = root_of(&state, &collection_id)?;
    logged(blocking(move || disk::read(&root, &file)).await)
}

/// Saves a journey; `file` is `None` for a new one. Returns its summary.
#[tauri::command]
#[specta::specta]
pub async fn save_journey(
    state: State<'_, StoreState>,
    collection_id: String,
    file: Option<String>,
    journey: Journey,
) -> JourneyResult<JourneySummary> {
    let root = root_of(&state, &collection_id)?;
    logged(
        blocking(move || {
            let name = disk::write(&root, file.as_deref(), &journey)?;
            Ok(JourneySummary::of(&name, &journey))
        })
        .await,
    )
}

#[tauri::command]
#[specta::specta]
pub async fn delete_journey(
    state: State<'_, StoreState>,
    collection_id: String,
    file: String,
) -> JourneyResult<()> {
    let root = root_of(&state, &collection_id)?;
    logged(blocking(move || disk::delete(&root, &file)).await)
}

/// Runs a saved journey with the selected environment; steps arrive as `journeyRunEvent`.
#[tauri::command]
#[specta::specta]
pub async fn run_journey(
    app: AppHandle,
    state: State<'_, StoreState>,
    engine: State<'_, HttpEngine>,
    vars: State<'_, VarsState>,
    runs: State<'_, JourneyRuns>,
    collection_id: String,
    file: String,
) -> JourneyResult<RunSummary> {
    let root = root_of(&state, &collection_id)?;
    let journey = {
        let file = file.clone();
        logged(blocking(move || disk::read(&root, &file)).await)?
    };
    let id = key(&collection_id, &file);
    let Some(handle) = runs.begin(&id) else {
        return Err(AppError::invalid_input("That journey is already running."));
    };
    let secrets = {
        let store = Arc::clone(&state.store);
        let service = vars.service.clone();
        let location = RequestLocation::Collection {
            collection_id: collection_id.clone(),
            path: String::new(),
        };
        blocking(move || {
            let guard = store.lock().unwrap_or_else(PoisonError::into_inner);
            let snapshot = service.snapshot(&guard, &location);
            Ok(secrets_of(&service.scopes(&snapshot)))
        })
        .await
    };
    let Ok(secrets) = secrets else {
        runs.end(&id);
        return logged(secrets.map(|_| unreachable_summary()));
    };
    let trusted = state.lock_store().scripts_trusted(&collection_id);
    let run_id = uuid::Uuid::new_v4().to_string();
    let (emit_collection, emit_file, emit_run) =
        (collection_id.clone(), file.clone(), run_id.clone());
    let emit_app = app.clone();
    let host = EngineHost::new(
        SendDeps {
            engine: &engine,
            store: &state,
            vars: &vars.service,
            dynamics: vars.dynamics.clone(),
        },
        collection_id,
        handle.cancel,
        handle.current,
        secrets,
        trusted,
        Box::new(move |event| {
            let message = JourneyRunEvent {
                run_id: emit_run.clone(),
                collection_id: emit_collection.clone(),
                file: emit_file.clone(),
                event,
            };
            if let Err(source) = message.emit(&emit_app) {
                tracing::warn!(%source, "could not emit journeyRunEvent");
            }
        }),
    );
    let summary = run(&host, &journey, &run_id).await;
    runs.end(&id);
    Ok(summary)
}

fn unreachable_summary() -> RunSummary {
    RunSummary {
        run_id: String::new(),
        status: super::types::RunStatus::Failed,
        steps: 0,
        failures: 0,
        message: String::new(),
    }
}

/// Stops a running journey and the request or script it is in. A journey that is not running
/// is a no-op.
#[tauri::command]
#[specta::specta]
pub fn cancel_journey(
    engine: State<'_, HttpEngine>,
    runs: State<'_, JourneyRuns>,
    collection_id: String,
    file: String,
) -> JourneyResult<()> {
    let Some(handle) = runs.handle(&key(&collection_id, &file)) else {
        return Ok(());
    };
    handle.cancel.store(true, Ordering::SeqCst);
    let current = handle
        .current
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    if let Some(id) = current {
        logged(engine.cancel(&id))?;
    }
    Ok(())
}
