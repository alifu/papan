//! Webhook IPC types (plan 07 §4).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::AppError;
use crate::store::types::{KeyValue, ScratchItem};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WebhookBind {
    #[default]
    Loopback,
    AllInterfaces,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct WebhookResponse {
    pub status: u32,
    pub headers: Vec<KeyValue>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct WebhookConfig {
    pub id: String,
    pub name: String,
    pub bind: WebhookBind,
    /// 0 means any free port.
    pub port: u32,
    pub path: String,
    pub response: WebhookResponse,
    pub max_body_bytes: u32,
    pub max_captures: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct WebhookListener {
    pub config: WebhookConfig,
    pub running: bool,
    pub url: Option<String>,
    pub captured: u32,
    pub unmatched: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct CaptureSummary {
    pub id: String,
    pub at_ms: f64,
    pub method: String,
    pub path: String,
    pub size: u32,
    pub content_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Capture {
    pub summary: CaptureSummary,
    pub query: String,
    pub headers: Vec<KeyValue>,
    pub remote: String,
    pub body_text: Option<String>,
    pub body_base64: Option<String>,
    pub truncated: bool,
}

/// Result of turning a capture into a scratch request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct ReplayResult {
    pub item: ScratchItem,
    pub warnings: Vec<String>,
}

/// Emitted for each captured request. TS: `events.webhookCaptured`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct WebhookCaptured {
    pub listener_id: String,
    pub capture: CaptureSummary,
}

/// Emitted when a listener stops on its own. TS: `events.webhookStopped`.
#[derive(Debug, Clone, PartialEq, Serialize, Type, tauri_specta::Event)]
pub struct WebhookStopped {
    pub listener_id: String,
    pub error: Option<AppError>,
}
