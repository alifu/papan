//! Variable resolution for a send (plan 04 §6): the scopes are built off the async
//! runtime (the first vault access may wait on a keychain prompt). The send itself runs
//! through the scripted pipeline in `scripted.rs` (plan 05 §4.2).

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::resolve::{resolve_content, Resolved};
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
