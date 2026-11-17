//! IPC types of the GraphQL helpers (plan 06 §8).

use serde::{Deserialize, Serialize};
use specta::Type;

/// Where a cached schema came from: an introspection result or an SDL file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SchemaSource {
    Introspection { json: String },
    Sdl { text: String },
}

/// A schema for one endpoint. `endpoint` is masked (scheme, host, port and path only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct GraphqlSchema {
    pub endpoint: String,
    pub fetched_at_ms: f64,
    pub source: SchemaSource,
}
