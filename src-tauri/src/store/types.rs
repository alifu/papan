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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum RequestLocation {
    Scratch { id: String },
    Collection { collection_id: String, path: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RequestDoc {
    pub name: String,
    pub method: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RequestContent {
    pub method: String,
    pub url: String,
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
