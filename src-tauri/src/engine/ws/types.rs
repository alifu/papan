//! WebSocket IPC types (plan 07 §4).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::AppError;
use crate::store::types::{KeyValue, WsFormat};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WsDirection {
    Sent,
    Received,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WsFrame {
    Text,
    Binary,
    Ping,
    Pong,
    Close,
}

/// One logged frame. `text` is base64 for `Binary`; secrets are masked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct WsMessage {
    pub seq: u32,
    pub at_ms: f64,
    pub direction: WsDirection,
    pub frame: WsFrame,
    pub text: String,
    pub size: u32,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct WsOpened {
    pub connection_id: String,
    /// Masked.
    pub url: String,
    pub protocol: Option<String>,
    pub status: u32,
    /// Response headers, `Set-Cookie` values masked.
    pub headers: Vec<KeyValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WsCloseBy {
    Client,
    Server,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsEventKind {
    Opened(WsOpened),
    Messages {
        items: Vec<WsMessage>,
    },
    Closed {
        code: Option<u32>,
        reason: String,
        by: WsCloseBy,
        error: Option<AppError>,
    },
}

/// Emitted per connection. TS: `events.wsEvent`.
#[derive(Debug, Clone, PartialEq, Serialize, Type, tauri_specta::Event)]
pub struct WsEvent {
    pub connection_id: String,
    pub kind: WsEventKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct WsOutgoing {
    pub frame: WsFormat,
    pub text: String,
}
