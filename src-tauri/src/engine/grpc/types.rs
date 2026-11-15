//! gRPC IPC types (plan 08 §4).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::engine::ws::types::WsMessage;
use crate::error::AppError;
use crate::store::types::{GrpcSource, KeyValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum GrpcMethodKind {
    Unary,
    ServerStreaming,
    ClientStreaming,
    Bidi,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct GrpcMethod {
    pub name: String,
    pub kind: GrpcMethodKind,
    pub input_type: String,
    pub output_type: String,
    /// Proto3 JSON request template with every field.
    pub template: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct GrpcService {
    pub full_name: String,
    pub methods: Vec<GrpcMethod>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct GrpcSchema {
    pub source: GrpcSource,
    pub services: Vec<GrpcService>,
    pub files: Vec<String>,
    pub warnings: Vec<String>,
    /// Reflection cache time.
    pub fetched_at_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct GrpcStarted {
    pub call_id: String,
    pub attempt: u32,
    /// `/pkg.Svc/Method`.
    pub path: String,
    pub kind: GrpcMethodKind,
    /// Masked.
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum GrpcEndBy {
    Client,
    Server,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct GrpcStatus {
    pub code: u32,
    /// `OK`, `UNAVAILABLE`, ...
    pub name: String,
    pub message: String,
    pub details_base64: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GrpcEventKind {
    Headers {
        metadata: Vec<KeyValue>,
    },
    Messages {
        items: Vec<WsMessage>,
    },
    Finished {
        status: GrpcStatus,
        trailers: Vec<KeyValue>,
        by: GrpcEndBy,
        elapsed_ms: f64,
        error: Option<AppError>,
    },
}

/// Emitted per call. TS: `events.grpcEvent`.
#[derive(Debug, Clone, PartialEq, Serialize, Type, tauri_specta::Event)]
pub struct GrpcEvent {
    pub call_id: String,
    pub attempt: u32,
    pub kind: GrpcEventKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ProtoImport {
    pub added: Vec<String>,
    pub warnings: Vec<String>,
}
