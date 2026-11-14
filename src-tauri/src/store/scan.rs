//! Scanning a collection's `requests/` into the tree shown in the sidebar (plan §3.4).

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use super::collection::{is_listable, read_folder_file, read_request_file, Collection, MAX_DEPTH};
use super::fsio::{self, DirEntry, EntryKind};
use super::paths::{self, FOLDER_FILE};
use super::protocol_model::parse_kind;
use super::types::{NodeKind, TreeNode};

/// Children in display order: names listed in `order` that exist, then the rest by name.
pub fn arrange(names: &[String], order: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let listed: Vec<String> = order
        .iter()
        .filter(|name| names.contains(name) && seen.insert((*name).clone()))
        .cloned()
        .collect();
    let rest: BTreeSet<&String> = names.iter().filter(|name| !seen.contains(*name)).collect();
    listed
        .into_iter()
        .chain(rest.into_iter().cloned())
        .collect()
}

fn invalid_node(rel: &str, entry: &str) -> TreeNode {
    TreeNode {
        path: paths::join_rel(rel, entry),
        name: entry.to_string(),
        kind: NodeKind::Invalid,
        method: None,
        request_kind: None,
        children: Vec::new(),
    }
}

impl Collection {
    /// The tree under `requests/` (empty when the folder does not exist yet).
    pub fn tree(&self) -> Vec<TreeNode> {
        let root = self.requests_root();
        if fsio::entry_kind(&root) != Some(EntryKind::Dir) {
            return Vec::new();
        }
        scan(&root, "", 0).1
    }
}

/// Scans one folder: its display name from `_folder.yaml` and its ordered children.
fn scan(dir: &Path, rel: &str, depth: usize) -> (Option<String>, Vec<TreeNode>) {
    let entries = fsio::list_dir(dir).unwrap_or_else(|error| {
        tracing::warn!(detail = ?error.detail, "could not list a collection folder");
        Vec::new()
    });
    let folder_file = entries
        .iter()
        .any(|entry| entry.name == FOLDER_FILE && entry.kind == EntryKind::File)
        .then(|| read_folder_file(dir));
    let mut nodes: HashMap<String, TreeNode> = entries
        .iter()
        .filter(|entry| is_listable(entry))
        .map(|entry| (entry.name.clone(), scan_entry(dir, rel, entry, depth)))
        .collect();
    let names: Vec<String> = nodes.keys().cloned().collect();
    let (display_name, order, broken) = match folder_file {
        Some(Ok(file)) => (file.name, file.order, None),
        Some(Err(_)) => (None, Vec::new(), Some(invalid_node(rel, FOLDER_FILE))),
        None => (None, Vec::new(), None),
    };
    let ordered = arrange(&names, &order)
        .into_iter()
        .filter_map(|name| nodes.remove(&name))
        .chain(broken)
        .collect();
    (display_name, ordered)
}

fn scan_entry(dir: &Path, rel: &str, entry: &DirEntry, depth: usize) -> TreeNode {
    let path = paths::join_rel(rel, &entry.name);
    let abs = dir.join(&entry.name);
    if entry.kind == EntryKind::Dir {
        let (name, children) = if depth + 1 < MAX_DEPTH {
            scan(&abs, &path, depth + 1)
        } else {
            tracing::warn!(%path, "collection is nested too deeply; not descending");
            (None, Vec::new())
        };
        return TreeNode {
            path,
            name: name.unwrap_or_else(|| entry.name.clone()),
            kind: NodeKind::Folder,
            method: None,
            request_kind: None,
            children,
        };
    }
    match read_request_file(&abs) {
        Ok(file) => TreeNode {
            request_kind: parse_kind(&entry.name, &file.kind).ok(),
            path,
            name: file.name,
            kind: NodeKind::Request,
            method: Some(file.method),
            children: Vec::new(),
        },
        Err(error) => {
            tracing::debug!(%path, "{error}");
            invalid_node(rel, &entry.name)
        }
    }
}
