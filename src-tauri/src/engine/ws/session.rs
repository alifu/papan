//! One live connection (plan 07 §5): the handshake over our own transport, then a task
//! that reads frames into the log and the [`WsInbound`], writes what the engine queues,
//! and runs the close handshake (5 s, then the stream is dropped).

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, WebSocketConfig};
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::{client_async_with_config, WebSocketStream};

use super::handshake::{masked_url, WsConnectSpec};
use super::log::{Entry, WsLog, MAX_PAYLOAD_BYTES};
use super::messages::{Masker, Outbound, Payload, MAX_OUTGOING_BYTES};
use super::sink::WsInbound;
use super::transport::{self, BoxedIo};
use super::types::{WsCloseBy, WsDirection, WsFrame, WsMessage, WsOpened};
use crate::engine::http::variables::now_ms;
use crate::error::{AppError, ErrorCode};
use crate::store::types::KeyValue;

pub const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);
pub const DEFAULT_CLOSE_CODE: u16 = 1000;
/// Frames queued for the writer before `send` waits.
pub const QUEUE_DEPTH: usize = 64;
/// Base64 of the bytes that fit the log's payload cap (plus one group, so a cut shows).
const LOGGED_BINARY_BYTES: usize = MAX_PAYLOAD_BYTES / 4 * 3 + 3;

pub type Socket = WebSocketStream<BoxedIo>;

pub type SharedLog = Arc<Mutex<WsLog>>;

pub fn lock_log(log: &Mutex<WsLog>) -> MutexGuard<'_, WsLog> {
    log.lock().unwrap_or_else(PoisonError::into_inner)
}

pub enum Command {
    Send(Message),
    Close { code: u16, reason: String },
}

fn ws_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(MAX_OUTGOING_BYTES))
        .max_frame_size(Some(MAX_OUTGOING_BYTES))
}

fn network(message: &str, detail: impl ToString) -> AppError {
    AppError::new(ErrorCode::Network, message).with_detail(detail.to_string())
}

/// Maps a tungstenite error to a message that names the cause without quoting any data.
pub fn ws_error(error: &WsError) -> AppError {
    match error {
        WsError::Http(response) => network(
            &format!(
                "The server refused the WebSocket upgrade (status {}).",
                response.status().as_u16()
            ),
            error,
        ),
        WsError::Capacity(_) => network(
            "A message was larger than the 16 MiB limit; the connection was closed.",
            error,
        ),
        WsError::Protocol(_) => network(
            "The server broke the WebSocket protocol (or chose a subprotocol that was not offered).",
            error,
        ),
        WsError::Utf8(_) => network("The server sent text that is not valid UTF-8.", error),
        WsError::Io(_) | WsError::ConnectionClosed | WsError::AlreadyClosed => {
            network("The connection was lost.", error)
        }
        _ => network("The WebSocket connection failed.", error),
    }
}

/// Headers the library owns: a user value would break the upgrade.
const RESERVED_HEADERS: [&str; 5] = [
    "connection",
    "upgrade",
    "sec-websocket-key",
    "sec-websocket-version",
    "sec-websocket-extensions",
];
/// Headers the user may replace rather than repeat.
const REPLACED_HEADERS: [&str; 2] = ["host", "sec-websocket-protocol"];

fn request_of(spec: &WsConnectSpec) -> Result<http::Request<()>, AppError> {
    let mut request = spec.url.as_str().into_client_request().map_err(|source| {
        AppError::invalid_input("The handshake request is not valid.")
            .with_detail(source.to_string())
    })?;
    for (name, value) in &spec.headers {
        if RESERVED_HEADERS.contains(&name.as_str()) {
            continue;
        }
        if REPLACED_HEADERS.contains(&name.as_str()) {
            request.headers_mut().insert(name.clone(), value.clone());
        } else {
            request.headers_mut().append(name.clone(), value.clone());
        }
    }
    Ok(request)
}

fn masked_cookie(value: &str) -> String {
    let attributes = value.split_once(';').map_or("", |(_, rest)| rest);
    let name = value.split('=').next().unwrap_or_default();
    format!("{name}=***;{attributes}")
}

fn response_headers(response: &http::Response<Option<Vec<u8>>>, masker: &Masker) -> Vec<KeyValue> {
    response
        .headers()
        .iter()
        .map(|(name, value)| {
            let text = String::from_utf8_lossy(value.as_bytes()).into_owned();
            let shown = if name.as_str() == "set-cookie" {
                masked_cookie(&text)
            } else {
                masker.mask(&text)
            };
            KeyValue {
                name: name.to_string(),
                value: shown,
                enabled: true,
            }
        })
        .collect()
}

/// TCP/proxy/TLS and the upgrade; the caller bounds it with the connect timeout and
/// can cancel it by dropping the future.
pub async fn connect(id: &str, spec: &WsConnectSpec) -> Result<(Socket, WsOpened), AppError> {
    let request = request_of(spec)?;
    let stream = transport::connect(&spec.url, spec.proxy.as_ref(), &spec.tls).await?;
    let (socket, response) = client_async_with_config(request, stream, Some(ws_config()))
        .await
        .map_err(|error| ws_error(&error))?;
    let masker = spec.masker();
    let protocol = response
        .headers()
        .get("sec-websocket-protocol")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let opened = WsOpened {
        connection_id: id.to_string(),
        url: masker.mask(&masked_url(&spec.url)),
        protocol,
        status: u32::from(response.status().as_u16()),
        headers: response_headers(&response, &masker),
    };
    Ok((socket, opened))
}

/// How a connection ended, for [`WsInbound::closed`].
struct Ended {
    code: Option<u32>,
    reason: String,
    by: WsCloseBy,
    error: Option<AppError>,
}

struct Context {
    log: SharedLog,
    inbound: Arc<dyn WsInbound>,
    masker: Masker,
}

impl Context {
    fn record(&self, frame: WsFrame, text: String, raw: &[u8]) {
        let entry = Entry {
            at_ms: now_ms(),
            direction: WsDirection::Received,
            frame,
            text,
            size: raw.len(),
        };
        let message = lock_log(&self.log).push(entry);
        self.inbound.message(&message, raw);
    }

    fn on_frame(&self, message: &Message) -> Option<(u16, String)> {
        match message {
            Message::Text(text) => {
                self.record(
                    WsFrame::Text,
                    self.masker.mask(text.as_str()),
                    text.as_bytes(),
                );
            }
            Message::Binary(bytes) => {
                let shown = &bytes[..bytes.len().min(LOGGED_BINARY_BYTES)];
                self.record(WsFrame::Binary, STANDARD.encode(shown), bytes);
            }
            Message::Ping(bytes) => self.record(WsFrame::Ping, lossy(bytes), bytes),
            Message::Pong(bytes) => self.record(WsFrame::Pong, lossy(bytes), bytes),
            Message::Close(frame) => {
                let (code, reason) = frame.as_ref().map_or((None, String::new()), |frame| {
                    (Some(u16::from(frame.code)), frame.reason.to_string())
                });
                let text = close_text(code, &reason);
                self.record(WsFrame::Close, text, reason.as_bytes());
                return Some((code.unwrap_or(1005), reason));
            }
            Message::Frame(_) => {}
        }
        None
    }
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn close_text(code: Option<u16>, reason: &str) -> String {
    match code {
        Some(code) => format!("{code} {reason}").trim_end().to_string(),
        None => String::new(),
    }
}

async fn wait_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

async fn run(mut socket: Socket, mut rx: mpsc::Receiver<Command>, cx: &Context) -> Ended {
    // `ours`: the close we sent; `theirs`: the close frame we received.
    let mut ours: Option<(u16, String)> = None;
    let mut theirs: Option<(u16, String)> = None;
    let mut deadline: Option<Instant> = None;
    let mut failure: Option<AppError> = None;
    loop {
        tokio::select! {
            command = rx.recv(), if ours.is_none() => {
                let (code, reason) = match command {
                    Some(Command::Send(message)) => {
                        if let Err(error) = socket.send(message).await {
                            failure = Some(ws_error(&error));
                            break;
                        }
                        continue;
                    }
                    Some(Command::Close { code, reason }) => (code, reason),
                    None => (DEFAULT_CLOSE_CODE, String::new()),
                };
                let frame = CloseFrame { code: CloseCode::from(code), reason: reason.clone().into() };
                if let Err(error) = socket.close(Some(frame)).await {
                    failure = Some(ws_error(&error));
                    break;
                }
                ours = Some((code, reason));
                deadline = Some(Instant::now() + CLOSE_TIMEOUT);
            }
            next = socket.next() => match next {
                Some(Ok(message)) => {
                    if let Some(received) = cx.on_frame(&message) {
                        theirs = Some(received);
                        deadline.get_or_insert_with(|| Instant::now() + CLOSE_TIMEOUT);
                    }
                    if matches!(message, Message::Ping(_)) {
                        // The pong is queued by the library; send it without waiting for a write.
                        let _ = socket.flush().await;
                    }
                }
                Some(Err(WsError::ConnectionClosed | WsError::AlreadyClosed)) | None => break,
                Some(Err(error)) => {
                    failure = Some(ws_error(&error));
                    break;
                }
            },
            () = wait_until(deadline) => break,
        }
    }
    ended(ours, theirs, failure)
}

fn ended(
    ours: Option<(u16, String)>,
    theirs: Option<(u16, String)>,
    failure: Option<AppError>,
) -> Ended {
    if let Some((code, reason)) = ours {
        return Ended {
            code: Some(u32::from(code)),
            reason,
            by: WsCloseBy::Client,
            error: failure,
        };
    }
    if let Some((code, reason)) = theirs {
        return Ended {
            code: Some(u32::from(code)),
            reason,
            by: WsCloseBy::Server,
            error: failure,
        };
    }
    match failure {
        Some(error) => Ended {
            code: None,
            reason: String::new(),
            by: WsCloseBy::Error,
            error: Some(error),
        },
        None => Ended {
            code: None,
            reason: String::new(),
            by: WsCloseBy::Server,
            error: None,
        },
    }
}

/// Spawns the session task reading `rx`; `on_end` runs after the closed event was
/// delivered.
pub fn spawn(
    socket: Socket,
    rx: mpsc::Receiver<Command>,
    log: SharedLog,
    inbound: Arc<dyn WsInbound>,
    masker: Masker,
    on_end: impl FnOnce() + Send + 'static,
) {
    tokio::spawn(async move {
        let cx = Context {
            log,
            inbound,
            masker,
        };
        let outcome = run(socket, rx, &cx).await;
        tracing::info!(code = ?outcome.code, by = ?outcome.by, "websocket closed");
        cx.inbound.closed(
            outcome.code,
            &outcome.reason,
            outcome.by,
            outcome.error.as_ref(),
        );
        on_end();
    });
}

/// Records a message the user sends and builds the frame for it.
pub fn outgoing(log: &SharedLog, outbound: Outbound) -> (WsMessage, Message) {
    let (frame, message, size) = match outbound.payload {
        Payload::Text(text) => {
            let size = text.len();
            (WsFrame::Text, Message::text(text), size)
        }
        Payload::Binary(bytes) => {
            let size = bytes.len();
            (WsFrame::Binary, Message::Binary(bytes.into()), size)
        }
    };
    let entry = Entry {
        at_ms: now_ms(),
        direction: WsDirection::Sent,
        frame,
        text: outbound.shown,
        size,
    };
    (lock_log(log).push(entry), message)
}
