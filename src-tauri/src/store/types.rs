//! Types shared with the frontend through the IPC bindings.

use serde::{Deserialize, Serialize};
use specta::Type;

/// An open collection as listed in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CollectionSummary {
    pub id: String,
    pub name: String,
    pub path: String,
    /// False when the collection folder is missing or unreadable.
    pub available: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Folder,
    Request,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TreeNode {
    /// Relative to `requests/`, `/`-separated.
    pub path: String,
    pub name: String,
    pub kind: NodeKind,
    /// Requests only.
    pub method: Option<String>,
    /// Folders only.
    pub children: Vec<TreeNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CollectionTree {
    pub collection: CollectionSummary,
    pub children: Vec<TreeNode>,
}

/// Result of a mutation: the new tree and the node's new path (`None` after delete).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TreeChange {
    pub tree: CollectionTree,
    pub path: Option<String>,
}

/// Where a request lives: in the app's scratch area or inside a collection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum RequestLocation {
    Scratch { id: String },
    Collection { collection_id: String, path: String },
}

/// A header, form field or other name/value row that can be switched off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct KeyValue {
    pub name: String,
    pub value: String,
    pub enabled: bool,
}

/// A query parameter switched off in the params table (plan 03 §3.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Pair {
    pub name: String,
    pub value: String,
}

/// A multipart part; a non-empty `file` makes it a file part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Part {
    pub name: String,
    pub value: String,
    pub file: Option<String>,
    pub enabled: bool,
}

/// `Unsupported` is a body type made by a newer Papan: kept on disk, never sent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Body {
    #[default]
    None,
    Json {
        text: String,
    },
    Xml {
        text: String,
    },
    Text {
        text: String,
    },
    Form {
        fields: Vec<KeyValue>,
    },
    Multipart {
        parts: Vec<Part>,
    },
    Binary {
        file: Option<String>,
    },
    Unsupported,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ApiKeyPlacement {
    #[default]
    Header,
    Query,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OAuthGrant {
    #[default]
    ClientCredentials,
    AuthorizationCode,
}

/// How the OAuth 2 client authenticates to the token endpoint.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ClientAuth {
    #[default]
    Basic,
    Body,
}

/// `Http2` is `h2` by ALPN over TLS and prior knowledge over cleartext (plan 03b §6.2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum HttpVersion {
    #[default]
    Auto,
    Http1,
    Http2,
}

/// `Unsupported` is an auth type made by a newer Papan: kept on disk, never sent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Auth {
    #[default]
    None,
    Basic {
        username: String,
        password: String,
    },
    Bearer {
        token: String,
    },
    ApiKey {
        name: String,
        value: String,
        placement: ApiKeyPlacement,
    },
    Digest {
        username: String,
        password: String,
    },
    /// `auth_url` is used by `AuthorizationCode` only; `redirect_port` 0 means any free port.
    Oauth2 {
        grant: OAuthGrant,
        auth_url: String,
        token_url: String,
        client_id: String,
        client_secret: String,
        scope: String,
        client_auth: ClientAuth,
        redirect_port: u16,
    },
    Unsupported,
}

/// Per-request send settings. `timeout_ms` 0 means no timeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Settings {
    pub follow_redirects: bool,
    pub timeout_ms: u32,
    pub verify_tls: bool,
    pub http_version: HttpVersion,
}

pub const DEFAULT_TIMEOUT_MS: u32 = 30_000;

impl Default for Settings {
    fn default() -> Self {
        Self {
            follow_redirects: true,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            verify_tls: true,
            http_version: HttpVersion::Auto,
        }
    }
}

/// Everything about a request except its name. Enabled query parameters live only in `url`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RequestContent {
    pub method: String,
    pub url: String,
    pub disabled_params: Vec<Pair>,
    pub headers: Vec<KeyValue>,
    pub body: Body,
    pub auth: Auth,
    pub settings: Settings,
}

impl RequestContent {
    /// Content with `method` and `url` and every section at its default.
    pub fn new(method: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            url: url.into(),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RequestDoc {
    pub name: String,
    pub content: RequestContent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ScratchItem {
    pub id: String,
    pub name: String,
    pub method: String,
}

/// Emitted when a collection changed on disk outside the app. TS: `events.collectionChanged`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct CollectionChanged {
    pub collection_id: String,
}
