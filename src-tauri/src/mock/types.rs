//! IPC types for mocks (plan 11 §6).

use serde::{Deserialize, Serialize};
use specta::Type;

use super::model::MockEnvironment;
use crate::error::AppError;
use crate::store::types::KeyValue;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct MockSummary {
    pub file: String,
    pub name: String,
    pub port: u16,
    pub routes: u32,
    pub running: bool,
    /// Where it listens right now.
    pub url: Option<String>,
    /// Why the file cannot be used, when it cannot.
    pub error: Option<String>,
}

impl MockSummary {
    pub fn of(file: &str, mock: &MockEnvironment) -> Self {
        Self {
            file: file.to_string(),
            name: mock.name.clone(),
            port: mock.port,
            routes: u32::try_from(mock.routes.len()).unwrap_or(u32::MAX),
            running: false,
            url: None,
            error: None,
        }
    }

    pub fn broken(file: &str, error: &str) -> Self {
        Self {
            file: file.to_string(),
            name: file.trim_end_matches(super::disk::SUFFIX).to_string(),
            port: 0,
            routes: 0,
            running: false,
            url: None,
            error: Some(error.to_string()),
        }
    }
}

/// What a mock import reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MockImportFormat {
    Mockers,
    Openapi,
}

/// A mock made from a file, with what could not be carried over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ImportedMock {
    pub summary: MockSummary,
    pub notes: Vec<String>,
}

/// One request the mock served.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct MockLogEntry {
    pub id: u32,
    pub at_ms: f64,
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: Vec<KeyValue>,
    pub body_text: String,
    pub body_truncated: bool,
    pub remote: String,
    pub route_id: Option<String>,
    pub response_id: Option<String>,
    pub status: u16,
    pub duration_ms: u32,
}

/// Emitted for each request a running mock served. TS: `events.mockLogged`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct MockLogged {
    pub collection_id: String,
    pub file: String,
    pub entry: MockLogEntry,
}

/// Emitted when a mock stops on its own. TS: `events.mockStopped`.
#[derive(Debug, Clone, PartialEq, Serialize, Type, tauri_specta::Event)]
pub struct MockStopped {
    pub collection_id: String,
    pub file: String,
    pub error: Option<AppError>,
}
