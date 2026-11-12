//! `send_request` with variables (plan 04 §6): build the scopes off the async runtime
//! (the first vault access may wait on a keychain prompt), resolve, send, and record
//! the original, unresolved draft in history.

use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use super::resolve::{resolve_content, Resolved};
use super::service::record_history;
use super::types::ResponseMeta;
use super::HttpEngine;
use crate::error::AppError;
use crate::store::types::{RequestContent, RequestLocation};
use crate::store::StoreState;
use crate::vars::dynamic::Dynamics;
use crate::vars::VarsService;

pub fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64() * 1000.0)
        .unwrap_or_default()
}

/// Resolves `content` for `location`. The store lock is released before the vault is
/// read; neither is held across a network await.
pub async fn resolve_for(
    store: &StoreState,
    vars: &VarsService,
    dynamics: Arc<dyn Dynamics>,
    location: &RequestLocation,
    content: &RequestContent,
) -> Result<Resolved, AppError> {
    let store = Arc::clone(&store.store);
    let vars = vars.clone();
    let location = location.clone();
    let content = content.clone();
    tokio::task::spawn_blocking(move || {
        let snapshot = {
            let guard = store
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            vars.snapshot(&guard, &location)
        };
        let scopes = vars.scopes(&snapshot);
        resolve_content(&content, &scopes, dynamics.as_ref())
    })
    .await
    .map_err(|source| {
        AppError::internal("Could not prepare the request.").with_detail(source.to_string())
    })
}

/// Sends `content` with its variables resolved and records it in history.
pub async fn send_with_variables(
    engine: &HttpEngine,
    store: &StoreState,
    vars: &VarsService,
    dynamics: Arc<dyn Dynamics>,
    request_id: &str,
    location: &RequestLocation,
    content: &RequestContent,
) -> Result<ResponseMeta, AppError> {
    let sent_at_ms = now_ms();
    let started = Instant::now();
    let result = match resolve_for(store, vars, dynamics, location, content).await {
        Ok(resolved) => engine.send_resolved(request_id, location, &resolved).await,
        Err(error) => Err(error),
    };
    record_history(
        store,
        location,
        content,
        sent_at_ms,
        started.elapsed(),
        &result,
    );
    result
}
