//! HTTP engine types shared with the frontend through the IPC bindings (plan 03 §8).
//! No `u64`/`i64` here: sizes and durations are `u32`, timestamps `f64` epoch milliseconds.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{AppError, ErrorCode};
use crate::script::ScriptReport;
use crate::store::types::RequestContent;
use crate::vars::types::{SecretEdit, SecretStorageStatus};

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
    pub timing: Timing,
    pub tls_verified: bool,
    /// Variable names that were sent as typed because they did not resolve.
    pub unresolved: Vec<String>,
}

/// What `send_request` returns (plan 05 §5). `meta` is `None` only when the
/// pre-request chain stopped the send (its report's `error` says why; nothing was sent).
/// A transport failure is an `Err`, unless a chain already ran: then it is `error` here, with
/// `meta` `None`, so the pre-request report is not lost. A post-response error keeps `meta`.
/// `scripts` holds one report per chain that ran, pre-request first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct SendOutcome {
    pub meta: Option<ResponseMeta>,
    pub scripts: Vec<ScriptReport>,
    pub error: Option<AppError>,
}

/// Where the time of a send went (plan 03b §6.4). `connect_ms` is TCP + TLS (+ proxy
/// `CONNECT`); `reused` means no new connection was opened. Redirect hops are summed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Timing {
    pub dns_ms: u32,
    pub connect_ms: u32,
    pub wait_ms: u32,
    pub download_ms: u32,
    pub reused: bool,
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

/// Machine-local proxy choice (plan 03b §4.1). `System` is the OS proxy configuration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    #[default]
    System,
    None,
    Custom,
}

/// The proxy as shown in Network settings; the password itself is never returned.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ProxySettings {
    pub mode: ProxyMode,
    pub url: String,
    pub username: String,
    pub has_password: bool,
    pub bypass: Vec<String>,
}

/// The proxy as written by Network settings; the password is write-only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ProxyInput {
    pub mode: ProxyMode,
    pub url: String,
    pub username: String,
    pub password: SecretEdit,
    pub bypass: Vec<String>,
}

/// An imported client certificate; the PEM bundle lives in the vault only.
/// `port` `None` matches any port; `missing` means the vault no longer holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct ClientCertificate {
    pub id: String,
    pub host: String,
    pub port: Option<u16>,
    pub cert_name: String,
    pub key_name: String,
    pub added_at_ms: f64,
    pub missing: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct NetworkSettings {
    pub proxy: ProxySettings,
    pub certificates: Vec<ClientCertificate>,
    pub storage: SecretStorageStatus,
}

/// A cookie in a jar, as listed by the cookie manager.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct StoredCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub expires_at_ms: Option<f64>,
    pub http_only: bool,
    pub secure: bool,
}

/// A cookie added or replaced by the cookie manager; `expires_at_ms` `None` is a session cookie.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct CookieInput {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub expires_at_ms: Option<f64>,
    pub http_only: bool,
    pub secure: bool,
}

/// Deletes every cookie of `domain`, or the one cookie named by domain + path + name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CookieFilter {
    pub domain: String,
    pub path: Option<String>,
    pub name: Option<String>,
}

/// A parsed cURL command and what was ignored in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CurlImport {
    pub content: RequestContent,
    pub warnings: Vec<String>,
}

/// OAuth 2 token state for one configuration (phase B, commands 47–50).
/// No field ever holds a token.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct OAuthStatus {
    pub config_key: String,
    pub has_token: bool,
    pub expires_at_ms: Option<f64>,
    pub can_refresh: bool,
    pub scope: String,
    pub auth_host: Option<String>,
    pub token_host: String,
    pub plaintext: bool,
    pub redirect_uri: Option<String>,
    /// Whether "Get new token" will verify the provider's TLS certificate.
    pub verify_tls: bool,
}
