//! Tauri commands for mocks (plan 11 §6). Thin adapters over `disk` and `MockService`.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_specta::Event;

use super::model::MockEnvironment;
use super::types::{
    ExportedMock, ImportedMock, MockImportFormat, MockLogEntry, MockLogged, MockSummary,
};
use super::{disk, MockService, MockSink};
use crate::error::AppError;
use crate::store::StoreState;

type MockResult<T> = Result<T, AppError>;

/// Forwards each served request to the webview as `mockLogged`.
pub struct TauriSink {
    app: AppHandle,
}

impl TauriSink {
    pub fn arc(app: AppHandle) -> Arc<dyn MockSink> {
        Arc::new(Self { app })
    }
}

impl MockSink for TauriSink {
    fn logged(&self, collection_id: &str, file: &str, entry: &MockLogEntry) {
        let event = MockLogged {
            collection_id: collection_id.to_string(),
            file: file.to_string(),
            entry: entry.clone(),
        };
        if let Err(source) = event.emit(&self.app) {
            tracing::warn!(%source, "could not emit mockLogged");
        }
    }
}

/// Collects every HTTP-like request of a collection (name, method, URL) in tree order.
fn sources_of(
    store: &mut crate::store::Store,
    collection_id: &str,
) -> MockResult<(String, PathBuf, Vec<super::from_collection::Source>)> {
    use crate::store::types::{NodeKind, RequestKind, RequestLocation, TreeNode};

    fn walk(nodes: &[TreeNode], out: &mut Vec<String>) {
        for node in nodes {
            match node.kind {
                NodeKind::Folder => walk(&node.children, out),
                NodeKind::Request
                    if matches!(
                        node.request_kind,
                        None | Some(RequestKind::Http | RequestKind::Graphql | RequestKind::Soap)
                    ) =>
                {
                    out.push(node.path.clone());
                }
                _ => {}
            }
        }
    }

    let tree = store.read_tree(collection_id)?;
    let root = store.collection_root(collection_id)?;
    let mut paths = Vec::new();
    walk(&tree.children, &mut paths);
    let sources = paths
        .into_iter()
        .filter_map(|path| {
            let location = RequestLocation::Collection {
                collection_id: collection_id.to_string(),
                path,
            };
            // A request that cannot be read is left out; the rest still make a mock.
            let doc = store.read_request(&location).ok()?;
            Some(super::from_collection::Source {
                name: doc.name,
                method: doc.content.method,
                url: doc.content.url,
            })
        })
        .collect();
    Ok((tree.collection.name, root, sources))
}

fn root_of(state: &StoreState, collection_id: &str) -> MockResult<PathBuf> {
    state.lock_store().collection_root(collection_id)
}

/// Mocks from a cloned collection may not expose the network or read files until the user
/// trusts the collection (the same switch as for its scripts).
fn check_trust(state: &StoreState, collection_id: &str, mock: &MockEnvironment) -> MockResult<()> {
    let Some(reason) = super::risky_features(mock) else {
        return Ok(());
    };
    if state.lock_store().scripts_trusted(collection_id) {
        return Ok(());
    }
    Err(AppError::invalid_input(format!(
        "This mock {reason}. It is in a collection that came from a Git repository, so it will not start until you trust the collection (collection menu, Trust scripts…)."
    )))
}

fn logged<T>(result: MockResult<T>) -> MockResult<T> {
    if let Err(error) = &result {
        tracing::warn!(code = ?error.code, detail = ?error.detail, "{error}");
    }
    result
}

async fn with_state(
    service: &MockService,
    collection_id: &str,
    mut summary: MockSummary,
) -> MockSummary {
    if let Some(url) = service.url_of(collection_id, &summary.file).await {
        summary.running = true;
        summary.url = Some(url);
    }
    summary
}

#[tauri::command]
#[specta::specta]
pub async fn list_mocks(
    state: State<'_, StoreState>,
    service: State<'_, MockService>,
    collection_id: String,
) -> MockResult<Vec<MockSummary>> {
    let root = root_of(&state, &collection_id)?;
    let listed = tauri::async_runtime::spawn_blocking(move || disk::list(&root))
        .await
        .map_err(|source| {
            AppError::internal("Could not list mocks.").with_detail(source.to_string())
        })?;
    let mut out = Vec::with_capacity(listed.len());
    for summary in listed {
        out.push(with_state(&service, &collection_id, summary).await);
    }
    Ok(out)
}

#[tauri::command]
#[specta::specta]
pub fn read_mock(
    state: State<'_, StoreState>,
    collection_id: String,
    file: String,
) -> MockResult<MockEnvironment> {
    let root = root_of(&state, &collection_id)?;
    logged(disk::read(&root, &file))
}

/// Saves a mock (a new file when `file` is null). A running mock restarts with the new
/// definition.
#[tauri::command]
#[specta::specta]
pub async fn save_mock(
    state: State<'_, StoreState>,
    service: State<'_, MockService>,
    collection_id: String,
    file: Option<String>,
    mock: MockEnvironment,
) -> MockResult<MockSummary> {
    let root = root_of(&state, &collection_id)?;
    let saved = mock.clone();
    let name = logged(disk::write(&root, file.as_deref(), &mock))?;
    if service.url_of(&collection_id, &name).await.is_some() {
        logged(check_trust(&state, &collection_id, &saved))?;
        service.stop(&collection_id, &name).await;
        logged(
            service
                .start(&collection_id, &name, root, saved.clone())
                .await,
        )?;
    }
    Ok(with_state(&service, &collection_id, MockSummary::of(&name, &saved)).await)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_mock(
    state: State<'_, StoreState>,
    service: State<'_, MockService>,
    collection_id: String,
    file: String,
) -> MockResult<()> {
    let root = root_of(&state, &collection_id)?;
    service.stop(&collection_id, &file).await;
    logged(disk::delete(&root, &file))
}

/// Starts the mock and returns its URL.
#[tauri::command]
#[specta::specta]
pub async fn start_mock(
    state: State<'_, StoreState>,
    service: State<'_, MockService>,
    collection_id: String,
    file: String,
) -> MockResult<String> {
    let root = root_of(&state, &collection_id)?;
    let mock = logged(disk::read(&root, &file))?;
    logged(check_trust(&state, &collection_id, &mock))?;
    logged(service.start(&collection_id, &file, root, mock).await)?;
    service
        .url_of(&collection_id, &file)
        .await
        .ok_or_else(|| AppError::internal("The mock stopped right after it started."))
}

#[tauri::command]
#[specta::specta]
pub async fn stop_mock(
    service: State<'_, MockService>,
    collection_id: String,
    file: String,
) -> MockResult<()> {
    service.stop(&collection_id, &file).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn mock_log(
    service: State<'_, MockService>,
    collection_id: String,
    file: String,
) -> MockResult<Vec<MockLogEntry>> {
    Ok(service.log(&collection_id, &file).await)
}

#[tauri::command]
#[specta::specta]
pub async fn clear_mock_log(
    service: State<'_, MockService>,
    collection_id: String,
    file: String,
) -> MockResult<()> {
    service.clear_log(&collection_id, &file).await;
    Ok(())
}

/// Writes a new mock with one route per HTTP, GraphQL and SOAP request of the collection.
#[tauri::command]
#[specta::specta]
pub async fn mock_from_collection(
    state: State<'_, StoreState>,
    collection_id: String,
) -> MockResult<MockSummary> {
    let store = Arc::clone(&state.store);
    let made = tauri::async_runtime::spawn_blocking(move || {
        let mut guard = store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (name, root, sources) = sources_of(&mut guard, &collection_id)?;
        drop(guard);
        let mock = super::from_collection::mock_from(&name, &sources);
        if mock.routes.is_empty() {
            return Err(AppError::invalid_input(
                "This collection has no HTTP, GraphQL or SOAP requests to mock.",
            ));
        }
        let file = disk::write(&root, None, &mock)?;
        Ok(MockSummary::of(&file, &mock))
    })
    .await
    .map_err(|source| {
        AppError::internal("Could not create the mock.").with_detail(source.to_string())
    })?;
    logged(made)
}

async fn dialog<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> MockResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|source| {
            AppError::internal("The file dialog failed.").with_detail(source.to_string())
        })
}

fn picked_path(picked: tauri_plugin_dialog::FilePath) -> MockResult<PathBuf> {
    picked.into_path().map_err(|source| {
        AppError::invalid_input("That file cannot be used.").with_detail(source.to_string())
    })
}

/// Picks a Mockers or OpenAPI file, converts it and saves it as a new mock; `None` when the
/// file dialog is cancelled.
#[tauri::command]
#[specta::specta]
pub async fn import_mock(
    app: AppHandle,
    state: State<'_, StoreState>,
    collection_id: String,
    format: MockImportFormat,
) -> MockResult<Option<ImportedMock>> {
    let root = root_of(&state, &collection_id)?;
    let picked = dialog(move || {
        app.dialog()
            .file()
            .add_filter("Mockers or OpenAPI", &["json", "yaml", "yml"])
            .blocking_pick_file()
    })
    .await?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = logged(picked_path(picked))?;
    let made = tauri::async_runtime::spawn_blocking(move || {
        let bytes = crate::store::fsio::read_bounded(&path)?;
        let text = String::from_utf8(bytes)
            .map_err(|_| AppError::invalid_input("That file is not valid UTF-8 text."))?;
        let converted = match format {
            MockImportFormat::Mockers => super::mockers::import(&text)?,
            MockImportFormat::Openapi => super::openapi::import(&text)?,
        };
        let file = disk::write(&root, None, &converted.mock)?;
        Ok(ImportedMock {
            summary: MockSummary::of(&file, &converted.mock),
            notes: converted.notes,
        })
    })
    .await
    .map_err(|source| {
        AppError::internal("Could not import the mock.").with_detail(source.to_string())
    })?;
    logged(made).map(Some)
}

/// Saves the mock as a Mockers environment file; returns the saved file's name and notes, or `None` when
/// the save dialog is cancelled.
#[tauri::command]
#[specta::specta]
pub async fn export_mock(
    app: AppHandle,
    state: State<'_, StoreState>,
    collection_id: String,
    file: String,
) -> MockResult<Option<ExportedMock>> {
    let root = root_of(&state, &collection_id)?;
    let mock = logged(disk::read(&root, &file))?;
    let (text, notes) = logged(super::mockers::export(&mock))?;
    let name = format!("{}.mockers.json", file.trim_end_matches(disk::SUFFIX));
    let picked =
        dialog(move || app.dialog().file().set_file_name(name).blocking_save_file()).await?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let target = logged(picked_path(picked))?;
    let saved = target
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    logged(
        tokio::fs::write(&target, text)
            .await
            .map_err(AppError::from),
    )?;
    Ok(Some(ExportedMock { file: saved, notes }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::types::{RequestContent, RequestKind, RequestLocation};
    use crate::store::Store;

    #[test]
    fn a_collection_becomes_a_mock_with_one_route_per_http_request() {
        let app = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let mut store = Store::new(app.path()).unwrap();
        let tree = store.create_collection(work.path(), "Shop").unwrap();
        let id = tree.collection.id.clone();
        // The folder name is slugified ("items"), so use the returned path, not "Items".
        let items = store.create_folder(&id, "", "Items").unwrap().path.unwrap();
        for (parent, name, kind, method, url) in [
            ("", "Health", RequestKind::Http, "GET", "{{base}}/health"),
            (
                items.as_str(),
                "Get item",
                RequestKind::Http,
                "GET",
                "{{base}}/items/{{id}}",
            ),
            (
                items.as_str(),
                "Socket",
                RequestKind::Ws,
                "GET",
                "wss://x.test/live",
            ),
        ] {
            let change = store.create_request(&id, parent, name, kind).unwrap();
            let location = RequestLocation::Collection {
                collection_id: id.clone(),
                path: change.path.unwrap(),
            };
            let content = RequestContent {
                kind,
                ..RequestContent::new(method, url)
            };
            store.write_request(&location, &content).unwrap();
        }

        let (name, root, sources) = sources_of(&mut store, &id).unwrap();
        let mock = super::super::from_collection::mock_from(&name, &sources);
        let routes: Vec<_> = mock
            .routes
            .iter()
            .map(|r| (r.method.as_str(), r.endpoint.as_str()))
            .collect();
        assert_eq!(routes.len(), 2, "{routes:?}");
        assert!(routes.contains(&("get", "health")));
        assert!(routes.contains(&("get", "items/:id")));
        let file = disk::write(&root, None, &mock).unwrap();
        assert_eq!(file, "shop-mock.mock.yaml");
        assert_eq!(disk::read(&root, &file).unwrap().routes.len(), 2);
    }
}
