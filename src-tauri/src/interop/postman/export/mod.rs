//! Postman export (plan 09 §3, §9 B1): our collection on disk → v2.1 JSON, plus
//! environments and globals. Reads request files and initial variable values only; secret
//! values are blank unless the caller passes them (read from the vault at export time).
//! Output is deterministic: fixed key order, tab indent, stable ids, trailing newline.

mod auth;
mod body;
mod collection;
mod request;
mod url;
mod vars;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::Serialize;

use super::types::{ReportItem, ReportLevel};
use crate::error::AppError;

pub use collection::{export_collection, ExportedCollection};
pub use request::request_item;
pub use vars::{collection_variables, environment, globals};

/// Secret values by variable name, read from the vault only when the user asked for them.
pub type SecretValues = BTreeMap<String, String>;

/// Suffixes of the written files (plan 09 §7).
pub const COLLECTION_SUFFIX: &str = ".postman_collection.json";
pub const ENVIRONMENT_SUFFIX: &str = ".postman_environment.json";
pub const GLOBALS_FILE: &str = "globals.postman_globals.json";

/// Pretty JSON with tab indent and a trailing newline: the bytes are stable for equal input.
pub fn to_json<T: Serialize>(value: &T) -> Result<String, AppError> {
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"\t");
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    value.serialize(&mut serializer).map_err(|error| {
        AppError::internal("Could not write the Postman file.").with_detail(error.to_string())
    })?;
    out.push(b'\n');
    String::from_utf8(out).map_err(|_| AppError::internal("Could not write the Postman file."))
}

/// Collects report entries for one output file.
#[derive(Debug, Default)]
pub struct Report {
    file: String,
    items: Vec<ReportItem>,
}

impl Report {
    pub fn new(file: &str) -> Self {
        Self {
            file: file.to_string(),
            items: Vec::new(),
        }
    }

    pub fn add(&mut self, level: ReportLevel, path: &str, message: impl Into<String>) {
        self.items.push(ReportItem {
            level,
            file: self.file.clone(),
            path: path.to_string(),
            message: message.into(),
        });
    }

    /// Adds `message` only the first time it is reported for this file.
    pub fn add_once(&mut self, level: ReportLevel, path: &str, message: &str) {
        if !self.items.iter().any(|item| item.message == message) {
            self.add(level, path, message);
        }
    }

    pub fn into_items(self) -> Vec<ReportItem> {
        self.items
    }
}
