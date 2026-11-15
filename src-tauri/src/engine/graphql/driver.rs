//! The `graphql-transport-ws` driver (plan 06b §5): a small state machine that is the
//! [`WsInbound`] of one subscription socket. It forwards everything to the inner sink
//! (so the M7 log and events are unchanged) and queues its replies for a writer task,
//! because the callbacks are synchronous.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;

use super::gql_ws::{self, ServerMessage, SUBPROTOCOL};
use crate::engine::ws::messages::{Masker, Outbound, Payload};
use crate::engine::ws::types::{WsCloseBy, WsDirection, WsFrame, WsMessage, WsOpened};
use crate::engine::ws::{WsEngine, WsInbound};
use crate::error::{AppError, ErrorCode};

pub const ACK_TIMEOUT: Duration = Duration::from_secs(10);
/// Replies waiting for the writer; a server that floods pings loses pongs, not memory.
const REPLY_QUEUE: usize = 16;
const CLOSE_NORMAL: u16 = 1000;
const CLOSE_PROTOCOL: u16 = 1002;
const CLOSE_BAD_REQUEST: u16 = 4400;
const CLOSE_ACK_TIMEOUT: u16 = 4504;
pub const WRONG_PROTOCOL: &str = "The server did not accept graphql-transport-ws. Legacy subscriptions-transport-ws (`graphql-ws`) servers are not supported.";

/// What the driver asks the writer to do.
#[derive(Debug)]
pub enum Reply {
    Send(Outbound),
    Close { code: u16, reason: &'static str },
}

pub type Ready = oneshot::Receiver<Result<(), AppError>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    AwaitingAck,
    Subscribed,
    Done,
}

struct Shared {
    phase: Phase,
    ready: Option<oneshot::Sender<Result<(), AppError>>>,
    timer: Option<AbortHandle>,
}

struct Inner {
    id: String,
    sink: Arc<dyn WsInbound>,
    replies: mpsc::Sender<Reply>,
    masker: Masker,
    /// Both hold user text that may carry secrets: never formatted or logged.
    init_frame: String,
    subscribe_frame: String,
    shared: Mutex<Shared>,
}

/// Cheap to clone; every clone drives the same subscription.
#[derive(Clone)]
pub struct GqlWsDriver {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for GqlWsDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GqlWsDriver")
            .field("id", &self.inner.id)
            .finish_non_exhaustive()
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn queue(&self, reply: Reply) {
        if self.replies.try_send(reply).is_err() {
            tracing::warn!(connection = %self.id, "subscription reply dropped");
        }
    }

    fn send_text(&self, text: String) {
        let shown = self.masker.mask(&text);
        self.queue(Reply::Send(Outbound {
            payload: Payload::Text(text),
            shown,
        }));
    }

    /// Ends the driver's work: stops the timer and tells whoever waits for the ack.
    fn finish(&self, outcome: Result<(), AppError>, phase: Phase) {
        let mut shared = self.lock();
        shared.phase = phase;
        if let Some(timer) = shared.timer.take() {
            timer.abort();
        }
        if let Some(ready) = shared.ready.take() {
            let _ = ready.send(outcome);
        }
    }

    fn fail_and_close(&self, error: AppError, code: u16, reason: &'static str) {
        self.finish(Err(error), Phase::Done);
        self.queue(Reply::Close { code, reason });
    }

    fn on_timeout(&self) {
        if self.lock().phase != Phase::AwaitingAck {
            return;
        }
        let error = AppError::new(
            ErrorCode::Timeout,
            "The server did not acknowledge the connection in 10 s.",
        );
        self.fail_and_close(error, CLOSE_ACK_TIMEOUT, "ack timeout");
    }

    fn on_message(&self, server: ServerMessage) {
        let phase = self.lock().phase;
        match (server, phase) {
            (_, Phase::Done) | (ServerMessage::Pong, _) => {}
            (ServerMessage::Ping(payload), _) => {
                self.send_text(gql_ws::pong(payload.as_ref()));
            }
            (ServerMessage::ConnectionAck, Phase::AwaitingAck) => {
                self.send_text(self.subscribe_frame.clone());
                self.finish(Ok(()), Phase::Subscribed);
            }
            (ServerMessage::ConnectionAck, Phase::Subscribed) => {
                self.finish(Ok(()), Phase::Done);
                self.queue(Reply::Close {
                    code: CLOSE_BAD_REQUEST,
                    reason: "duplicate connection_ack",
                });
            }
            (ServerMessage::Next { .. }, Phase::Subscribed) => {}
            (ServerMessage::Error { .. } | ServerMessage::Complete { .. }, Phase::Subscribed) => {
                self.finish(Ok(()), Phase::Done);
                self.queue(Reply::Close {
                    code: CLOSE_NORMAL,
                    reason: "operation ended",
                });
            }
            (_, Phase::AwaitingAck) => {
                let error = AppError::new(
                    ErrorCode::Network,
                    "The server sent a result before acknowledging the connection.",
                );
                self.fail_and_close(error, CLOSE_BAD_REQUEST, "message before ack");
            }
        }
    }

    fn on_invalid(&self, error: AppError) {
        tracing::warn!(connection = %self.id, "subscription protocol violation");
        let phase = self.lock().phase;
        if phase != Phase::Done {
            self.fail_and_close(error, CLOSE_BAD_REQUEST, "invalid message");
        }
    }

    fn closed_before_ready(
        &self,
        code: Option<u32>,
        reason: &str,
        error: Option<&AppError>,
    ) -> AppError {
        if let Some(error) = error {
            return error.clone();
        }
        let known = code
            .and_then(|code| u16::try_from(code).ok())
            .and_then(gql_ws::close_message);
        let message = match (known, code) {
            (Some(known), _) => known.to_string(),
            (None, Some(code)) => {
                let shown = format!("{code} {}", self.masker.mask(reason));
                format!(
                    "The server closed the connection before the subscription started ({}).",
                    shown.trim()
                )
            }
            (None, None) => "The connection closed before the subscription started.".to_string(),
        };
        AppError::new(ErrorCode::Network, message)
    }
}

impl GqlWsDriver {
    /// `init_payload` is checked JSON (or `None`); `body_json` is the subscribe payload.
    /// Replies come out of the returned receiver; [`spawn_writer`] consumes it.
    pub fn new(
        id: &str,
        sink: Arc<dyn WsInbound>,
        init_payload: Option<&str>,
        body_json: &str,
        masker: Masker,
    ) -> (Self, mpsc::Receiver<Reply>, Ready) {
        let (replies, rx) = mpsc::channel(REPLY_QUEUE);
        let (ready_tx, ready) = oneshot::channel();
        let inner = Inner {
            id: id.to_string(),
            sink,
            replies,
            masker,
            init_frame: gql_ws::connection_init(init_payload),
            subscribe_frame: gql_ws::subscribe(id, body_json),
            shared: Mutex::new(Shared {
                phase: Phase::AwaitingAck,
                ready: Some(ready_tx),
                timer: None,
            }),
        };
        (
            Self {
                inner: Arc::new(inner),
            },
            rx,
            ready,
        )
    }
}

impl WsInbound for GqlWsDriver {
    fn opened(&self, opened: &WsOpened) {
        let inner = &self.inner;
        inner.sink.opened(opened);
        if opened.protocol.as_deref() != Some(SUBPROTOCOL) {
            let error = AppError::new(ErrorCode::Network, WRONG_PROTOCOL);
            inner.fail_and_close(error, CLOSE_PROTOCOL, "unsupported subprotocol");
            return;
        }
        inner.send_text(inner.init_frame.clone());
        let timed = Arc::clone(&self.inner);
        let timer = tokio::spawn(async move {
            tokio::time::sleep(ACK_TIMEOUT).await;
            timed.on_timeout();
        });
        let mut shared = inner.lock();
        if shared.phase == Phase::AwaitingAck {
            shared.timer = Some(timer.abort_handle());
        } else {
            timer.abort();
        }
    }

    fn message(&self, message: &WsMessage, raw: &[u8]) {
        self.inner.sink.message(message, raw);
        if message.direction != WsDirection::Received || message.frame != WsFrame::Text {
            return;
        }
        let Ok(text) = std::str::from_utf8(raw) else {
            return;
        };
        match gql_ws::parse(text, &self.inner.id) {
            Ok(server) => self.inner.on_message(server),
            Err(error) => self.inner.on_invalid(error),
        }
    }

    fn closed(&self, code: Option<u32>, reason: &str, by: WsCloseBy, error: Option<&AppError>) {
        let inner = &self.inner;
        inner.sink.closed(code, reason, by, error);
        let pending = inner.lock().ready.is_some();
        if pending {
            let failure = inner.closed_before_ready(code, reason, error);
            inner.finish(Err(failure), Phase::Done);
        } else {
            inner.finish(Ok(()), Phase::Done);
        }
    }
}

/// Carries the driver's replies to the engine until the driver is gone.
pub fn spawn_writer(engine: WsEngine, id: String, mut replies: mpsc::Receiver<Reply>) {
    tokio::spawn(async move {
        while let Some(reply) = replies.recv().await {
            let sent = match reply {
                Reply::Send(outbound) => engine.send(&id, outbound).await.map(|_| ()),
                Reply::Close { code, reason } => engine.close_raw(&id, code, reason).await,
            };
            if let Err(error) = sent {
                tracing::debug!(connection = %id, code = ?error.code, "subscription reply not delivered");
            }
        }
    });
}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
