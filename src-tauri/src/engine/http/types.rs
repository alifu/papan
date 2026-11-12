//! HTTP engine types shared with the frontend through the IPC bindings (plan 03 §8).
//! No `u64`/`i64` here: sizes and durations are `u32`, timestamps `f64` epoch milliseconds.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::ErrorCode;

/// A response header; non-UTF-8 values are decoded lossily.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ResponseCookie {
    pub name: String,
    pub value: String,
    pub domain: Option<String>,
    pub path: Option<String>,
    pub expires: Option<String>,
    pub http_only: bool,
    pub secure: bool,
}

/// Everything about a response except its body, which is read with `read_response_body`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ResponseMeta {
    pub request_id: String,
    pub status: u16,
    pub status_text: String,
    pub http_version: String,
    /// Final URL after redirects.
    pub url: String,
    pub headers: Vec<Header>,
    pub cookies: Vec<ResponseCookie>,
    pub content_type: Option<String>,
    pub size_bytes: u32,
    pub truncated: bool,
    pub ttfb_ms: u32,
    pub total_ms: u32,
    pub tls_verified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BodyEncoding {
    Text,
    Base64,
    None,
}

/// A capped preview of a response body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ResponseBody {
    pub encoding: BodyEncoding,
    pub data: String,
    pub truncated: bool,
}

/// A file chosen in the native open dialog; `size_bytes` saturates at `u32::MAX`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PickedFile {
    pub path: String,
    pub name: String,
    pub size_bytes: u32,
}

/// One sent request as listed in the History panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct HistoryEntry {
    pub id: String,
    pub sent_at_ms: f64,
    pub name: String,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub error_code: Option<ErrorCode>,
    pub total_ms: u32,
    pub size_bytes: u32,
}
