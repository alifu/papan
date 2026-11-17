//! `<app data>/history.json`: the last sends, newest first (plan 03 §7). Snapshots are
//! redacted and large body text is dropped; response bodies are never stored.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::fsio;
use super::secrets::redact;
use super::types::{Body, RequestContent, RequestKind, RequestLocation, SoapSettings};
use crate::engine::http::types::HistoryEntry;
use crate::error::{AppError, ErrorCode};

pub const MAX_HISTORY: usize = 200;
pub const MAX_SNAPSHOT_BODY_BYTES: usize = 64 * 1024;
const HISTORY_FILE: &str = "history.json";
const HISTORY_VERSION: u32 = 1;

/// One stored send: the listed fields plus where it came from and what was sent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryRecord {
    #[serde(flatten)]
    pub entry: HistoryEntry,
    pub location: RequestLocation,
    pub content: RequestContent,
}

/// How a send ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Response {
        status: u16,
        total_ms: u32,
        size_bytes: u32,
    },
    Failed {
        code: ErrorCode,
        total_ms: u32,
    },
}

/// What a send needs to become a history record.
#[derive(Debug, Clone)]
pub struct Sent<'a> {
    pub location: &'a RequestLocation,
    pub name: &'a str,
    pub content: &'a RequestContent,
    pub sent_at_ms: f64,
    pub outcome: Outcome,
}

#[derive(Debug, Serialize, Deserialize)]
struct HistoryFile {
    version: u32,
    entries: Vec<HistoryRecord>,
}

/// `redact(content)` with body text over [`MAX_SNAPSHOT_BODY_BYTES`] emptied.
pub fn snapshot(content: &RequestContent) -> RequestContent {
    let redacted = redact(content);
    let body = match redacted.body {
        Body::Json { text } if text.len() > MAX_SNAPSHOT_BODY_BYTES => Body::Json {
            text: String::new(),
        },
        Body::Xml { text } if text.len() > MAX_SNAPSHOT_BODY_BYTES => Body::Xml {
            text: String::new(),
        },
        Body::Text { text } if text.len() > MAX_SNAPSHOT_BODY_BYTES => Body::Text {
            text: String::new(),
        },
        Body::Graphql {
            query,
            variables,
            operation_name,
        } if query.len() + variables.len() > MAX_SNAPSHOT_BODY_BYTES => Body::Graphql {
            query: String::new(),
            variables: String::new(),
            operation_name,
        },
        other => other,
    };
    RequestContent { body, ..redacted }
}

impl HistoryRecord {
    pub fn new(sent: &Sent<'_>) -> Self {
        let (status, error_code, total_ms, size_bytes) = match sent.outcome {
            Outcome::Response {
                status,
                total_ms,
                size_bytes,
            } => (Some(status), None, total_ms, size_bytes),
            Outcome::Failed { code, total_ms } => (None, Some(code), total_ms, 0),
        };
        Self {
            entry: HistoryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                sent_at_ms: sent.sent_at_ms,
                name: sent.name.to_string(),
                method: sent.content.method.clone(),
                url: sent.content.url.clone(),
                status,
                error_code,
                total_ms,
                size_bytes,
                kind: sent.content.kind,
            },
            location: sent.location.clone(),
            content: snapshot(sent.content),
        }
    }
}

/// Records written before M6 have no `kind` and no `content.kind`/`content.soap`:
/// they were HTTP sends, so the defaults are filled in. Present values are kept.
fn with_m6_defaults(mut file: serde_json::Value) -> serde_json::Value {
    let fill = |object: &mut serde_json::Map<String, serde_json::Value>, key: &str, value| {
        object.entry(key).or_insert(value);
    };
    let entries = file.get_mut("entries").and_then(|e| e.as_array_mut());
    for entry in entries.into_iter().flatten() {
        let Some(entry) = entry.as_object_mut() else {
            continue;
        };
        fill(entry, "kind", serde_json::json!(RequestKind::Http));
        if let Some(content) = entry.get_mut("content").and_then(|c| c.as_object_mut()) {
            fill(content, "kind", serde_json::json!(RequestKind::Http));
            fill(content, "soap", serde_json::json!(SoapSettings::default()));
            // M5-M8 records have scripts without the `enabled` flag (they always ran).
            if let Some(scripts) = content.get_mut("scripts").and_then(|s| s.as_object_mut()) {
                fill(scripts, "enabled", serde_json::json!(true));
            }
        }
    }
    file
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History {
    file: PathBuf,
}

impl History {
    pub fn new(app_data: &Path) -> Self {
        Self {
            file: app_data.join(HISTORY_FILE),
        }
    }

    /// Stored records, newest first; a missing or malformed file counts as empty.
    fn load(&self) -> Vec<HistoryRecord> {
        if fsio::entry_kind(&self.file).is_none() {
            return Vec::new();
        }
        fsio::read_bounded(&self.file)
            .and_then(|bytes| {
                serde_json::from_slice::<serde_json::Value>(&bytes)
                    .map(with_m6_defaults)
                    .and_then(serde_json::from_value::<HistoryFile>)
                    .map_err(|source| {
                        AppError::invalid_input("history.json is malformed.")
                            .with_detail(source.to_string())
                    })
            })
            .map(|file| file.entries)
            .unwrap_or_else(|error| {
                tracing::warn!(detail = ?error.detail, "{error} Starting with no history.");
                Vec::new()
            })
    }

    /// Writes `entries`, dropping the oldest while the file would exceed the read limit.
    fn save(&self, entries: &[HistoryRecord]) -> Result<(), AppError> {
        let encode = |entries: &[HistoryRecord]| {
            serde_json::to_vec(&HistoryFile {
                version: HISTORY_VERSION,
                entries: entries.to_vec(),
            })
            .map_err(|source| {
                AppError::internal("Could not write the history.").with_detail(source.to_string())
            })
        };
        let limit = usize::try_from(fsio::MAX_FILE_BYTES).unwrap_or(usize::MAX);
        let fitting = (0..=entries.len())
            .rev()
            .map(|count| encode(&entries[..count]))
            .find(|bytes| !matches!(bytes, Ok(bytes) if bytes.len() > limit))
            .unwrap_or_else(|| encode(&[]))?;
        fsio::write_atomic(&self.file, &fitting).map(|_| ())
    }

    pub fn append(&self, record: HistoryRecord) -> Result<(), AppError> {
        let entries: Vec<HistoryRecord> = std::iter::once(record)
            .chain(self.load())
            .take(MAX_HISTORY)
            .collect();
        self.save(&entries)
    }

    pub fn list(&self) -> Vec<HistoryEntry> {
        self.load().into_iter().map(|record| record.entry).collect()
    }

    pub fn clear(&self) -> Result<(), AppError> {
        self.save(&[])
    }

    pub fn find(&self, id: &str) -> Result<HistoryRecord, AppError> {
        self.load()
            .into_iter()
            .find(|record| record.entry.id == id)
            .ok_or_else(|| AppError::not_found("That history entry no longer exists."))
    }
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
