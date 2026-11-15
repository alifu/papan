//! Postman IPC types (plan 09 §6).

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PostmanFileKind {
    CollectionV20,
    CollectionV21,
    Environment,
    Globals,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PostmanFileSummary {
    /// File name only, never a path.
    pub file: String,
    pub kind: PostmanFileKind,
    pub name: String,
    pub requests: u32,
    pub folders: u32,
    pub variables: u32,
    pub scripts: u32,
    pub secret_candidates: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ReportLevel {
    Info,
    Warning,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ReportItem {
    pub level: ReportLevel,
    pub file: String,
    /// JSON path or tree path.
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PostmanPreview {
    pub token: String,
    pub files: Vec<PostmanFileSummary>,
    pub report: Vec<ReportItem>,
    pub truncated: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImportTarget {
    NewCollection,
    Collection { collection_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum GlobalsClash {
    Keep,
    Overwrite,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ImportOptions {
    pub target: ImportTarget,
    /// The user's final choice of secret variable names.
    pub secret_names: Vec<String>,
    pub store_secret_values: bool,
    pub import_scripts: bool,
    pub globals_clash: GlobalsClash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PostmanImportResult {
    pub collection_id: Option<String>,
    pub environments: Vec<String>,
    pub globals: u32,
    pub report: Vec<ReportItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ExportOptions {
    pub collection_id: String,
    pub environments: Vec<String>,
    pub include_globals: bool,
    pub include_secret_values: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PostmanExportResult {
    /// File names written.
    pub files: Vec<String>,
    pub report: Vec<ReportItem>,
}
