//! WebSocket engine built on `tokio-tungstenite` over our own transport (plan 07 §5).
//!
//! [`WsEngine`] is plain Rust, with no Tauri types: a connection is opened from a
//! [`WsConnectSpec`] and reports to a [`WsInbound`]. The commands bridge it to the
//! webview; GraphQL subscriptions (M6b) bring their own `WsInbound`.

pub mod commands;
pub mod handshake;
pub mod log;
pub mod messages;
pub mod session;
pub mod sink;
pub mod transport;
pub mod types;

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::{mpsc, oneshot};

pub use handshake::WsConnectSpec;
pub use messages::Outbound;
pub use sink::WsInbound;

use self::log::WsLog;
use self::session::{lock_log, Command, SharedLog, DEFAULT_CLOSE_CODE, QUEUE_DEPTH};
use self::types::{WsMessage, WsOpened};
use crate::engine::http::build::loggable_url;
use crate::engine::http::responses::validate_id;
use crate::error::{AppError, ErrorCode};

pub const MAX_CONNECTIONS: usize = 20;
/// Logs of closed connections kept so that a re-opened tab can show them.
pub const MAX_CLOSED_LOGS: usize = 20;
const MAX_REASON_BYTES: usize = 123;

enum State {
    Connecting(Option<oneshot::Sender<()>>),
    Open(mpsc::Sender<Command>),
    Closed,
}

struct Entry {
    log: SharedLog,
    state: State,
}

#[derive(Default)]
struct Registry {
    entries: HashMap<String, Entry>,
    closed: VecDeque<String>,
}

impl Registry {
    fn live(&self) -> usize {
        let is_live = |entry: &&Entry| match &entry.state {
            State::Connecting(_) => true,
            State::Open(tx) => !tx.is_closed(),
            State::Closed => false,
        };
        self.entries.values().filter(is_live).count()
    }

    /// Makes `id` open with `tx`. `false` when a `close` already took its cancel handle
    /// while it was connecting, so the connection must not open.
    fn install(&mut self, id: &str, tx: mpsc::Sender<Command>) -> bool {
        let Some(entry) = self.entries.get_mut(id) else {
            return true;
        };
        if matches!(entry.state, State::Connecting(None)) {
            return false;
        }
        entry.state = State::Open(tx);
        true
    }

    fn mark_closed(&mut self, id: &str) {
        let Some(entry) = self.entries.get_mut(id) else {
            return;
        };
        entry.state = State::Closed;
        self.closed.push_back(id.to_string());
        while self.closed.len() > MAX_CLOSED_LOGS {
            if let Some(oldest) = self.closed.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }
}

#[derive(Default)]
pub struct WsEngine {
    registry: Arc<Mutex<Registry>>,
}

fn lock(registry: &Mutex<Registry>) -> MutexGuard<'_, Registry> {
    registry.lock().unwrap_or_else(PoisonError::into_inner)
}

fn unknown() -> AppError {
    AppError::not_found("That WebSocket connection does not exist.")
}

fn closed_error() -> AppError {
    AppError::new(ErrorCode::Network, "The connection is closed.")
}

fn mask(error: AppError, spec: &WsConnectSpec) -> AppError {
    let masker = spec.masker();
    AppError {
        message: masker.mask(&error.message),
        detail: error.detail.as_deref().map(|detail| masker.mask(detail)),
        ..error
    }
}

impl WsEngine {
    /// Connects and starts the session. `id` is chosen by the caller and names the
    /// connection in every other call. Cancelled by [`Self::close`] while connecting.
    pub async fn open(
        &self,
        id: &str,
        spec: WsConnectSpec,
        inbound: Arc<dyn WsInbound>,
    ) -> Result<WsOpened, AppError> {
        let id = validate_id(id).map_err(|error| AppError {
            message: "That connection id is not valid.".into(),
            ..error
        })?;
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let log = self.reserve(&id, cancel_tx)?;
        tracing::info!(connection = %id, url = %loggable_url(&spec.url), "websocket connecting");
        let connected = self.connect(&id, &spec, cancel_rx).await;
        let (socket, opened) = match connected {
            Ok(connected) => connected,
            Err(error) => {
                lock(&self.registry).entries.remove(&id);
                tracing::warn!(connection = %id, code = ?error.code, "websocket not opened");
                return Err(mask(error, &spec));
            }
        };
        let (tx, rx) = mpsc::channel(QUEUE_DEPTH);
        if !lock(&self.registry).install(&id, tx) {
            // A `close` landed after the connect finished: it wins.
            lock(&self.registry).entries.remove(&id);
            return Err(AppError::new(
                ErrorCode::Cancelled,
                "The connection was cancelled.",
            ));
        }
        inbound.opened(&opened);
        let registry = Arc::clone(&self.registry);
        let ended = id.clone();
        let on_end = move || lock(&registry).mark_closed(&ended);
        session::spawn(socket, rx, log, inbound, spec.masker(), on_end);
        tracing::info!(connection = %id, status = opened.status, "websocket open");
        Ok(opened)
    }

    fn reserve(&self, id: &str, cancel: oneshot::Sender<()>) -> Result<SharedLog, AppError> {
        let mut registry = lock(&self.registry);
        let in_use = registry
            .entries
            .get(id)
            .is_some_and(|entry| !matches!(entry.state, State::Closed));
        if in_use {
            return Err(AppError::invalid_input(
                "That connection is already in use. Disconnect it first.",
            ));
        }
        if registry.live() >= MAX_CONNECTIONS {
            return Err(AppError::invalid_input(format!(
                "At most {MAX_CONNECTIONS} WebSocket connections can be open at once."
            )));
        }
        registry.closed.retain(|closed| closed != id);
        let log: SharedLog = Arc::new(Mutex::new(WsLog::default()));
        registry.entries.insert(
            id.to_string(),
            Entry {
                log: Arc::clone(&log),
                state: State::Connecting(Some(cancel)),
            },
        );
        Ok(log)
    }

    async fn connect(
        &self,
        id: &str,
        spec: &WsConnectSpec,
        cancel: oneshot::Receiver<()>,
    ) -> Result<(session::Socket, WsOpened), AppError> {
        let attempt = async {
            match spec.timeout {
                None => session::connect(id, spec).await,
                Some(limit) => tokio::time::timeout(limit, session::connect(id, spec))
                    .await
                    .unwrap_or_else(|_| {
                        Err(AppError::new(
                            ErrorCode::Timeout,
                            "The connection timed out.",
                        ))
                    }),
            }
        };
        tokio::select! {
            result = attempt => result,
            Ok(()) = cancel => Err(AppError::new(ErrorCode::Cancelled, "The connection was cancelled.")),
        }
    }

    /// Queues `outbound` and returns it as logged. No variables are resolved here.
    pub async fn send(&self, id: &str, outbound: Outbound) -> Result<WsMessage, AppError> {
        let (tx, log) = {
            let registry = lock(&self.registry);
            let entry = registry.entries.get(id).ok_or_else(unknown)?;
            let State::Open(tx) = &entry.state else {
                return Err(closed_error());
            };
            (tx.clone(), Arc::clone(&entry.log))
        };
        let permit = tx.reserve().await.map_err(|_| closed_error())?;
        let (logged, message) = session::outgoing(&log, outbound);
        permit.send(Command::Send(message));
        Ok(logged)
    }

    /// Starts the close handshake (`code` defaults to 1000), or cancels a connect in
    /// progress. A connection that is already closed is not an error.
    pub async fn close(&self, id: &str, code: Option<u32>, reason: &str) -> Result<(), AppError> {
        let code = close_code(code)?;
        if reason.len() > MAX_REASON_BYTES {
            return Err(AppError::invalid_input(
                "The close reason is longer than 123 bytes.",
            ));
        }
        let tx = {
            let mut registry = lock(&self.registry);
            let entry = registry.entries.get_mut(id).ok_or_else(unknown)?;
            match &mut entry.state {
                State::Connecting(cancel) => {
                    if let Some(cancel) = cancel.take() {
                        let _ = cancel.send(());
                    }
                    return Ok(());
                }
                State::Open(tx) => tx.clone(),
                State::Closed => return Ok(()),
            }
        };
        let command = Command::Close {
            code,
            reason: reason.to_string(),
        };
        // A full queue waits; a finished session has nothing left to close.
        let _ = tx.send(command).await;
        Ok(())
    }

    pub fn log(&self, id: &str) -> Result<Vec<WsMessage>, AppError> {
        let log = self.log_of(id)?;
        let snapshot = lock_log(&log).snapshot();
        Ok(snapshot)
    }

    pub fn clear_log(&self, id: &str) -> Result<(), AppError> {
        let log = self.log_of(id)?;
        lock_log(&log).clear();
        Ok(())
    }

    fn log_of(&self, id: &str) -> Result<SharedLog, AppError> {
        let registry = lock(&self.registry);
        let entry = registry.entries.get(id).ok_or_else(unknown)?;
        Ok(Arc::clone(&entry.log))
    }

    /// Connections that are open or connecting.
    pub fn open_count(&self) -> usize {
        lock(&self.registry).live()
    }
}

/// 1000, 1001 or an application code (3000 to 4999); the rest are reserved by RFC 6455.
fn close_code(code: Option<u32>) -> Result<u16, AppError> {
    let Some(code) = code else {
        return Ok(DEFAULT_CLOSE_CODE);
    };
    match u16::try_from(code) {
        Ok(code @ (1000 | 1001 | 3000..=4999)) => Ok(code),
        _ => Err(AppError::invalid_input(
            "The close code must be 1000, 1001 or between 3000 and 4999.",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_codes_are_limited_to_those_a_client_may_send() {
        assert_eq!(close_code(None).unwrap(), 1000);
        assert_eq!(close_code(Some(1001)).unwrap(), 1001);
        assert_eq!(close_code(Some(4000)).unwrap(), 4000);
        for bad in [0, 999, 1005, 1006, 2999, 5000, 70_000] {
            assert!(close_code(Some(bad)).is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn unknown_ids_are_not_found_and_a_bad_reason_is_refused() {
        let engine = WsEngine::default();

        assert_eq!(engine.log("nope").unwrap_err().code, ErrorCode::NotFound);
        assert_eq!(
            engine.close("nope", None, "").await.unwrap_err().code,
            ErrorCode::NotFound
        );
        let long = "r".repeat(MAX_REASON_BYTES + 1);
        assert!(engine.close("nope", None, &long).await.is_err());
        assert_eq!(
            engine
                .send("nope", Outbound::text("x"))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn closed_logs_are_kept_up_to_the_cap_and_the_oldest_is_dropped() {
        let mut registry = Registry::default();
        for n in 0..=MAX_CLOSED_LOGS {
            let id = format!("c{n}");
            registry.entries.insert(
                id.clone(),
                Entry {
                    log: Arc::default(),
                    state: State::Closed,
                },
            );
            registry.mark_closed(&id);
        }

        assert_eq!(registry.entries.len(), MAX_CLOSED_LOGS);
        assert!(!registry.entries.contains_key("c0"));
    }

    #[tokio::test]
    async fn a_close_after_the_connect_finished_still_wins() {
        let engine = WsEngine::default();
        let (cancel, _kept) = oneshot::channel();
        engine.reserve("c1", cancel).unwrap();
        engine.close("c1", None, "").await.unwrap();
        let (tx, _rx) = mpsc::channel(1);

        let installed = lock(&engine.registry).install("c1", tx);

        assert!(!installed);
    }
}
