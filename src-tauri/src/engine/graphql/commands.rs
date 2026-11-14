//! Tauri commands for GraphQL schemas (plan 06 §8). Thin adapters over
//! [`GraphqlDeps`]; the store lock is never held across a network await.

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use super::introspection::{too_large, MAX_SCHEMA_BYTES};
use super::service::GraphqlDeps;
use super::types::GraphqlSchema;
use super::GraphqlState;
use crate::engine::http::HttpEngine;
use crate::error::AppError;
use crate::store::types::{RequestContent, RequestLocation};
use crate::store::StoreState;
use crate::vars::commands::VarsState;

type GraphqlResult<T> = Result<T, AppError>;

const SCHEMA_EXTENSIONS: [&str; 3] = ["graphql", "graphqls", "json"];

fn logged<T>(result: GraphqlResult<T>) -> GraphqlResult<T> {
    if let Err(error) = &result {
        tracing::warn!(code = ?error.code, detail = ?error.detail, "{error}");
    }
    result
}

fn deps<'a>(
    engine: &'a HttpEngine,
    store: &'a StoreState,
    vars: &'a VarsState,
    graphql: &'a GraphqlState,
) -> GraphqlDeps<'a> {
    GraphqlDeps {
        engine,
        store,
        vars: &vars.service,
        dynamics: vars.dynamics.clone(),
        cache: &graphql.cache,
    }
}

/// Runs the introspection query against the request's endpoint and caches the schema.
/// Cancellable with `cancel_request(request_id)`.
#[tauri::command]
#[specta::specta]
pub async fn introspect_graphql(
    engine: State<'_, HttpEngine>,
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    graphql: State<'_, GraphqlState>,
    request_id: String,
    location: RequestLocation,
    content: RequestContent,
) -> GraphqlResult<GraphqlSchema> {
    let deps = deps(&engine, &store, &vars, &graphql);
    logged(deps.introspect(&request_id, &location, &content).await)
}

/// The cached schema for the request's resolved endpoint, without network.
#[tauri::command]
#[specta::specta]
pub async fn read_cached_schema(
    engine: State<'_, HttpEngine>,
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    graphql: State<'_, GraphqlState>,
    location: RequestLocation,
    content: RequestContent,
) -> GraphqlResult<Option<GraphqlSchema>> {
    let deps = deps(&engine, &store, &vars, &graphql);
    logged(deps.read_cached(&location, &content).await)
}

/// Reads a picked file of at most 20 MiB. The path is used once and never stored.
/// Only a regular file is read (checked on the opened handle, not the path), and never
/// more than the limit plus one byte, so a growing file, a FIFO or a device cannot make
/// the read unbounded.
pub(super) async fn read_schema_file(path: std::path::PathBuf) -> GraphqlResult<Vec<u8>> {
    use tokio::io::AsyncReadExt as _;
    let file = tokio::fs::File::open(&path).await.map_err(AppError::from)?;
    if !file.metadata().await.map_err(AppError::from)?.is_file() {
        return Err(AppError::invalid_input("Choose a regular schema file."));
    }
    let limit = u64::try_from(MAX_SCHEMA_BYTES).unwrap_or(u64::MAX);
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .await
        .map_err(AppError::from)?;
    if bytes.len() > MAX_SCHEMA_BYTES {
        return Err(too_large());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_schema_file_is_read_up_to_the_limit_and_must_be_a_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("s.graphql");
        let big = dir.path().join("b.json");
        std::fs::write(&small, b"type Query { a: Int }").unwrap();
        std::fs::write(&big, vec![b' '; MAX_SCHEMA_BYTES + 1]).unwrap();

        let read = read_schema_file(small).await.unwrap();
        let too_big = read_schema_file(big).await.unwrap_err();
        let folder = read_schema_file(dir.path().to_path_buf())
            .await
            .unwrap_err();

        assert_eq!(read, b"type Query { a: Int }");
        assert!(too_big.message.contains("20 MiB"), "{}", too_big.message);
        assert!(folder.message.contains("regular"), "{}", folder.message);
    }
}

/// Picks an SDL or introspection JSON file in a native dialog; `None` when cancelled.
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub async fn load_schema_file(
    app: AppHandle,
    engine: State<'_, HttpEngine>,
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    graphql: State<'_, GraphqlState>,
    location: RequestLocation,
    content: RequestContent,
) -> GraphqlResult<Option<GraphqlSchema>> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("GraphQL schema", &SCHEMA_EXTENSIONS)
            .blocking_pick_file()
    })
    .await
    .map_err(|source| {
        AppError::internal("The file dialog failed.").with_detail(source.to_string())
    })?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = logged(picked.into_path().map_err(|source| {
        AppError::invalid_input("That file cannot be used.").with_detail(source.to_string())
    }))?;
    let bytes = logged(read_schema_file(path).await)?;
    let deps = deps(&engine, &store, &vars, &graphql);
    logged(deps.store_file(&location, &content, &bytes).await).map(Some)
}

/// Deletes the cached schema of the request's resolved endpoint.
#[tauri::command]
#[specta::specta]
pub async fn clear_cached_schema(
    engine: State<'_, HttpEngine>,
    store: State<'_, StoreState>,
    vars: State<'_, VarsState>,
    graphql: State<'_, GraphqlState>,
    location: RequestLocation,
    content: RequestContent,
) -> GraphqlResult<()> {
    let deps = deps(&engine, &store, &vars, &graphql);
    logged(deps.clear_cached(&location, &content).await)
}
