//! IPC types for journeys (plan 13 §4).

use serde::{Deserialize, Serialize};
use specta::Type;

use super::model::Journey;

/// A journey file as listed in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct JourneySummary {
    pub file: String,
    pub name: String,
    pub nodes: u32,
    /// Why the file cannot be used, when it cannot.
    pub error: Option<String>,
}

impl JourneySummary {
    pub fn of(file: &str, journey: &Journey) -> Self {
        Self {
            file: file.to_string(),
            name: journey.name.clone(),
            nodes: u32::try_from(journey.nodes.len()).unwrap_or(u32::MAX),
            error: None,
        }
    }

    pub fn broken(file: &str, problem: &str) -> Self {
        Self {
            file: file.to_string(),
            name: file.to_string(),
            nodes: 0,
            error: Some(problem.to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Passed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RunEventKind {
    NodeStarted,
    NodeFinished,
    RunFinished,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct HeaderLine {
    pub name: String,
    pub value: String,
}

/// What a Request node got back: a preview only; the body is cut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ResponseSummary {
    pub status: u16,
    pub headers: Vec<HeaderLine>,
    pub body_preview: String,
    pub body_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct VarSet {
    pub name: String,
    pub value: String,
}

/// One step of a run. Fields that do not belong to the `kind` are empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RunEvent {
    pub kind: RunEventKind,
    pub node_id: String,
    pub status: Option<NodeStatus>,
    pub message: String,
    pub response: Option<ResponseSummary>,
    /// Variables the node set.
    pub vars: Vec<VarSet>,
    /// Console lines of a Script node.
    pub console: Vec<String>,
    /// Steps taken so far.
    pub steps: u32,
    /// Set on `run_finished`.
    pub run_status: Option<RunStatus>,
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RunSummary {
    pub run_id: String,
    pub status: RunStatus,
    pub steps: u32,
    pub failures: u32,
    pub message: String,
}

/// Emitted for each step of a running journey. TS: `events.journeyRunEvent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct JourneyRunEvent {
    pub run_id: String,
    pub collection_id: String,
    pub file: String,
    pub event: RunEvent,
}
