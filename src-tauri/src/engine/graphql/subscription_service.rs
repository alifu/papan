//! Opening and stopping one subscription (plan 06b §5, §6). Plain Rust over [`WsEngine`]
//! and [`HttpEngine`], so integration tests drive it without Tauri.
//!
//! A `graphql_unsubscribe` that races the connect must win: it sets a flag the connect
//! checks after every step, and also closes the socket if the engine already has one.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::driver::{spawn_writer, GqlWsDriver, WRONG_PROTOCOL};
use super::gql_ws;
use super::subscription::{frames, subscription_spec};
use crate::engine::http::resolve::Resolved;
use crate::engine::http::HttpEngine;
use crate::engine::ws::types::WsOpened;
use crate::engine::ws::{Outbound, WsEngine, WsInbound};
use crate::error::{AppError, ErrorCode};
use crate::store::types::{RequestContent, RequestLocation};

const CLOSE_NORMAL: u16 = 1000;

/// Subscriptions that are still connecting or waiting for the ack, by connection id.
#[derive(Default)]
pub struct Subscriptions {
    pending: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

fn cancelled() -> AppError {
    AppError::new(ErrorCode::Cancelled, "The subscription was stopped.")
}

impl Subscriptions {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Arc<AtomicBool>>> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn register(&self, id: &str) -> Result<Arc<AtomicBool>, AppError> {
        let mut pending = self.lock();
        if pending.contains_key(id) {
            return Err(AppError::invalid_input(
                "That subscription is already starting. Stop it first.",
            ));
        }
        let flag = Arc::new(AtomicBool::new(false));
        pending.insert(id.to_string(), Arc::clone(&flag));
        Ok(flag)
    }

    /// Opens the socket, runs the handshake and resolves after `connection_ack` and the
    /// queued `subscribe`. `typed` is the draft before resolution.
    #[allow(clippy::too_many_arguments)] // one service per argument; no natural grouping
    pub async fn subscribe(
        &self,
        engine: &WsEngine,
        http: &HttpEngine,
        id: &str,
        location: &RequestLocation,
        typed: &RequestContent,
        resolved: &Resolved,
        sink: Arc<dyn WsInbound>,
        version: &str,
    ) -> Result<WsOpened, AppError> {
        let stop = self.register(id)?;
        let result = run(
            engine, http, id, location, typed, resolved, sink, version, &stop,
        )
        .await;
        self.lock().remove(id);
        result
    }

    /// Sends `complete` and closes with 1000; cancels a subscription that is still
    /// starting. A subscription that is already closed is not an error.
    pub async fn unsubscribe(&self, engine: &WsEngine, id: &str) -> Result<(), AppError> {
        let starting = self.lock().get(id).cloned();
        if let Some(flag) = &starting {
            flag.store(true, Ordering::SeqCst);
        } else {
            let _ = engine.send(id, Outbound::text(gql_ws::complete(id))).await;
        }
        match engine.close_raw(id, CLOSE_NORMAL, "").await {
            Err(error) if error.code != ErrorCode::NotFound => Err(error),
            _ => Ok(()),
        }
    }
}

/// A server that picks a subprotocol we did not offer (legacy `graphql-ws`) or none at
/// all makes the handshake fail; say what that means here.
fn legacy_hint(error: AppError) -> AppError {
    if error.code == ErrorCode::Network && error.message.contains("subprotocol") {
        return AppError::new(ErrorCode::Network, WRONG_PROTOCOL);
    }
    error
}

#[allow(clippy::too_many_arguments)]
async fn run(
    engine: &WsEngine,
    http: &HttpEngine,
    id: &str,
    location: &RequestLocation,
    typed: &RequestContent,
    resolved: &Resolved,
    sink: Arc<dyn WsInbound>,
    version: &str,
    stop: &AtomicBool,
) -> Result<WsOpened, AppError> {
    let built = frames(resolved)?;
    let spec = subscription_spec(http, location, typed, resolved, version).await?;
    if stop.load(Ordering::SeqCst) {
        return Err(cancelled());
    }
    let (driver, replies, ready) = GqlWsDriver::new(
        id,
        sink,
        built.init_payload.as_deref(),
        &built.body_json,
        spec.masker(),
    );
    spawn_writer(engine.clone(), id.to_string(), replies);
    let opened = engine
        .open(id, spec, Arc::new(driver))
        .await
        .map_err(legacy_hint)?;
    let outcome = ready.await;
    if stop.load(Ordering::SeqCst) {
        // A stop that landed between the engine's states: make sure the socket ends.
        let _ = engine.close_raw(id, CLOSE_NORMAL, "").await;
        return Err(cancelled());
    }
    match outcome {
        Ok(Ok(())) => Ok(opened),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(AppError::new(
            ErrorCode::Network,
            "The subscription ended before it started.",
        )),
    }
}
