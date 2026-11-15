//! gRPC engine built on `tonic` and `prost-reflect` (plan 08 §5).
//!
//! [`GrpcEngine`] is plain Rust, with no Tauri types: it owns the running calls and
//! their logs. A call is named by an id the caller chooses (stable per request
//! location) and an `attempt` that grows with every start, so events of an older attempt
//! can be told apart. Race rules: a start on a running id cancels it and waits for its
//! `finished`; a cancel always wins over a start that has not opened its call yet.

pub mod cache;
pub mod call;
pub mod channel;
pub mod codec;
pub mod commands;
pub mod connect;
pub mod import;
pub mod metadata;
pub mod protos;
pub mod reflection;
mod registry;
pub mod schema;
pub mod service;
pub mod sink;
pub mod types;

use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use futures_util::stream::{self, StreamExt as _};
use prost_reflect::{DynamicMessage, MethodDescriptor};
use tokio::sync::{mpsc, oneshot, watch};

pub use self::call::Transport;
pub use self::connect::GrpcCallSpec;
pub use self::sink::GrpcInbound;

use self::call::{record, CallTask, Requests};
use self::codec::from_json;
use self::registry::{Registry, Running};
use self::types::{GrpcMethodKind, GrpcStarted};
use crate::engine::http::responses::validate_id;
use crate::engine::ws::log::WsLog;
use crate::engine::ws::messages::Masker;
use crate::engine::ws::session::{lock_log, SharedLog};
use crate::engine::ws::types::{WsDirection, WsMessage};
use crate::error::{AppError, ErrorCode};

pub const MAX_CALLS: usize = 20;
/// Logs of finished calls kept so that a re-opened tab can show them.
pub const MAX_CLOSED_LOGS: usize = 20;
/// Messages queued for a client or bidi stream before `send` waits.
pub const QUEUE_DEPTH: usize = 64;

/// A call ready to start: descriptors found, channel connected, message parsed.
pub struct Prepared {
    pub transport: Box<dyn Transport>,
    pub method: MethodDescriptor,
    /// The request of a unary or server-streaming call; client and bidi streams start
    /// empty.
    pub first: Option<DynamicMessage>,
    pub deadline: Option<Duration>,
    pub masker: Masker,
    /// Masked.
    pub url: String,
}

#[derive(Default, Clone)]
pub struct GrpcEngine {
    registry: Arc<Mutex<Registry>>,
}

fn lock(registry: &Mutex<Registry>) -> MutexGuard<'_, Registry> {
    registry.lock().unwrap_or_else(PoisonError::into_inner)
}

fn cancelled() -> AppError {
    AppError::new(ErrorCode::Cancelled, "The call was cancelled.")
}

fn ended() -> AppError {
    AppError::invalid_input("The call has ended.")
}

fn takes_stream(kind: GrpcMethodKind) -> bool {
    matches!(kind, GrpcMethodKind::ClientStreaming | GrpcMethodKind::Bidi)
}

impl GrpcEngine {
    /// Starts a call under `id`. `prepare` does everything that can fail or take time
    /// before the request is sent (resolve, descriptors, connect) and is dropped when
    /// [`Self::cancel`] arrives first; `sink_for` gets the attempt number of this start.
    /// A call still running under `id` is cancelled and has reported `finished` before
    /// `prepare` is polled.
    pub async fn start<F>(
        &self,
        id: &str,
        prepare: F,
        sink_for: impl FnOnce(u32) -> Arc<dyn GrpcInbound>,
    ) -> Result<GrpcStarted, AppError>
    where
        F: Future<Output = Result<Prepared, AppError>>,
    {
        let id = validate_id(id).map_err(|error| AppError {
            message: "That call id is not valid.".into(),
            ..error
        })?;
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let (attempt, previous) = lock(&self.registry).reserve(&id, cancel_tx)?;
        let work = async {
            if let Some(mut done) = previous {
                // An error only means the old task is gone already.
                let _ = done.wait_for(|finished| *finished).await;
            }
            prepare.await
        };
        let prepared = tokio::select! {
            prepared = work => prepared,
            _ = cancel_rx => Err(cancelled()),
        };
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                lock(&self.registry).abandon(&id, attempt);
                tracing::warn!(call = %id, attempt, code = ?error.code, "grpc call not started");
                return Err(error);
            }
        };
        self.launch(&id, attempt, prepared, sink_for)
    }

    fn launch(
        &self,
        id: &str,
        attempt: u32,
        prepared: Prepared,
        sink_for: impl FnOnce(u32) -> Arc<dyn GrpcInbound>,
    ) -> Result<GrpcStarted, AppError> {
        let kind = schema::method_kind(&prepared.method);
        let path = schema::path_of(&prepared.method);
        let (tx, requests): (_, Requests) = match prepared.first.clone() {
            Some(first) => (None, stream::once(async move { first }).boxed()),
            None => {
                let (tx, rx) = mpsc::channel(QUEUE_DEPTH);
                let rest = stream::unfold(rx, |mut rx| async {
                    rx.recv().await.map(|message| (message, rx))
                });
                (Some(tx), rest.boxed())
            }
        };
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let (done_tx, done_rx) = watch::channel(false);
        let log: SharedLog = Arc::new(Mutex::new(WsLog::default()));
        let running = Running {
            cancel: Some(cancel_tx),
            tx: tx.filter(|_| takes_stream(kind)),
            input: prepared.method.input(),
            kind,
            masker: prepared.masker.clone(),
        };
        let installed =
            lock(&self.registry).install(id, attempt, running, Arc::clone(&log), done_rx);
        if !installed {
            // A cancel or a newer start landed after `prepare` finished: it wins.
            lock(&self.registry).abandon(id, attempt);
            return Err(cancelled());
        }
        let inbound = sink_for(attempt);
        if let Some(first) = &prepared.first {
            let mask = |text: &str| prepared.masker.mask(text);
            let sent = record(&log, mask, WsDirection::Sent, first);
            inbound.message(&sent);
        }
        tracing::info!(call = %id, attempt, url = %prepared.url, %path, ?kind, "grpc call started");
        let task = CallTask {
            id: id.to_string(),
            attempt,
            transport: prepared.transport,
            requests,
            deadline: prepared.deadline,
            masker: prepared.masker,
            log,
            inbound,
            cancel: cancel_rx,
        };
        let (registry, ended_id) = (Arc::clone(&self.registry), id.to_string());
        tokio::spawn(async move {
            // A panic in the call must still free the slot.
            if let Err(error) = tokio::spawn(call::run(task)).await {
                tracing::error!(call = %ended_id, attempt, "grpc call task failed: {error}");
            }
            lock(&registry).finish(&ended_id, attempt);
            let _ = done_tx.send(true);
        });
        Ok(GrpcStarted {
            call_id: id.to_string(),
            attempt,
            path,
            kind,
            url: prepared.url,
        })
    }

    /// Queues one more request message (JSON, variables already resolved) on a client
    /// or bidi stream and returns it as logged. `secrets` masks what this message used
    /// beyond the secrets of the call.
    pub async fn send(
        &self,
        id: &str,
        json: &str,
        secrets: &Masker,
    ) -> Result<WsMessage, AppError> {
        let (tx, input, masker, log) = {
            let registry = lock(&self.registry);
            let (running, log) = registry.running(id).ok_or_else(ended)?;
            if !takes_stream(running.kind) {
                return Err(AppError::invalid_input(
                    "This method takes one message, and it was sent at the start.",
                ));
            }
            let tx = running.tx.clone().ok_or_else(|| {
                AppError::invalid_input("The request stream has ended. Start the call again.")
            })?;
            (tx, running.input.clone(), running.masker.clone(), log)
        };
        let mask = |text: &str| secrets.mask(&masker.mask(text));
        let message = from_json(&input, json).map_err(|error| AppError {
            message: mask(&error.message),
            ..error
        })?;
        let permit = tx.reserve().await.map_err(|_| ended())?;
        // `end` may have run while the permit was awaited: the check and the send share
        // one lock, so nothing goes out after the half-close.
        let registry = lock(&self.registry);
        let open = registry
            .running(id)
            .is_some_and(|(running, _)| running.tx.as_ref().is_some_and(|t| t.same_channel(&tx)));
        if !open {
            return Err(AppError::invalid_input(
                "The request stream has ended. Start the call again.",
            ));
        }
        let logged = record(&log, mask, WsDirection::Sent, &message);
        permit.send(message);
        drop(registry);
        Ok(logged)
    }

    /// Half-closes the request stream. Ending twice is not an error.
    pub fn end(&self, id: &str) -> Result<(), AppError> {
        let mut registry = lock(&self.registry);
        let running = registry.running_mut(id).ok_or_else(ended)?;
        running.tx = None;
        Ok(())
    }

    /// Cancels the call, or a start that has not opened it yet. A call that has
    /// finished, or an id that was never started, is not an error.
    pub fn cancel(&self, id: &str) {
        lock(&self.registry).cancel(id);
    }

    /// The log of the latest call under `id`; empty when there was none.
    pub fn log(&self, id: &str) -> Vec<WsMessage> {
        let log = lock(&self.registry).log_of(id);
        log.map(|log| lock_log(&log).snapshot()).unwrap_or_default()
    }

    pub fn clear_log(&self, id: &str) {
        if let Some(log) = lock(&self.registry).log_of(id) {
            lock_log(&log).clear();
        }
    }

    /// Calls that are starting or running.
    pub fn live_count(&self) -> usize {
        lock(&self.registry).live()
    }
}

#[cfg(test)]
mod tests;
