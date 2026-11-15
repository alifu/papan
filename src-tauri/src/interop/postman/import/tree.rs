//! A Postman collection to an in-memory tree of folder and request files (plan 09 §3, §4).
//! Names never become paths: every directory and file name is `unique_stem(slugify(name))`
//! among its siblings, checked again by `is_valid_segment`. Order is kept in `_folder.yaml`.

use std::collections::BTreeMap;

use super::events::plan_scripts;
use super::request::{disable_scripts, map_request, request_file, with_description, At};
use super::secrets::Lifter;
use super::vars::{from_collection, plan_vars, PlannedVars};
use crate::error::AppError;
use crate::interop::postman::limits::{
    clean_display_name, file_error, MAX_FOLDER_DEPTH, MAX_ITEMS, MAX_VARIABLES,
};
use crate::interop::postman::report::Report;
use crate::interop::postman::schema;
use crate::store::env_model::VarEntry;
use crate::store::model::{CollectionFile, FolderFile};
use crate::store::paths::{self, REQUEST_SUFFIX};
use crate::store::types::{RequestContent, Scripts};

/// Folder levels below the collection root. Two short of the store's limit: an import
/// into an open collection adds one wrapping folder, and requests need a level of their own.
pub const MAX_IMPORT_FOLDER_DEPTH: usize = MAX_FOLDER_DEPTH - 2;
const UNTITLED_REQUEST: &str = "Untitled request";
const UNTITLED_FOLDER: &str = "Untitled folder";
const UNTITLED_COLLECTION: &str = "Imported collection";
const POSTMAN_ID: &str = "postman_id";

#[derive(Debug, Clone, PartialEq)]
pub enum PlannedNode {
    Folder(PlannedFolder),
    Request {
        entry: String,
        file: crate::store::model::RequestFile,
    },
}

/// A folder: its directory name (empty for the requests root), `_folder.yaml` and children.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedFolder {
    pub dir: String,
    pub file: FolderFile,
    pub children: Vec<PlannedNode>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlannedCollection {
    pub source: String,
    pub name: String,
    /// Name, `postman_id`, description and (switched off) scripts; no variables.
    pub file: CollectionFile,
    /// Collection variables, including lifted credentials (always secret).
    pub vars: PlannedVars,
    /// Names of the variables made from literal credentials (renamed on a clash when
    /// importing into an open collection).
    pub lifted: Vec<String>,
    pub root: PlannedFolder,
    pub requests: u32,
    pub folders: u32,
    pub scripts: u32,
}

/// Plans `collection` read from `source` (a file name).
pub fn plan_collection(
    source: &str,
    collection: &schema::Collection,
    report: &mut Report,
) -> Result<PlannedCollection, AppError> {
    let items = count_items(&collection.item);
    if items > MAX_ITEMS {
        return Err(file_error(
            source,
            &format!("has {items} items; at most {MAX_ITEMS} can be imported."),
        ));
    }
    let (name, _) = clean_display_name(&collection.info.name, UNTITLED_COLLECTION);
    let vars = plan_vars(
        source,
        "variable",
        &from_collection(&collection.variable),
        report,
    );
    let room = MAX_VARIABLES.saturating_sub(vars.table.len());
    let mut walker = Walker {
        source,
        report,
        lifter: Lifter::new(vars.table.keys().cloned(), room),
        inherits: BTreeMap::new(),
        requests: 0,
        folders: 0,
        scripts: 0,
    };
    let scripts = walker.scripts("event", &collection.event);
    let inherited = Inherited {
        auth: collection.auth.as_ref(),
        from: "the collection".into(),
    };
    let children = walker.children("item", &collection.item, &inherited, 0);
    let root = PlannedFolder {
        dir: String::new(),
        file: FolderFile::default().with_order(entries(&children)),
        children,
    };
    let file = collection_file(&name, collection, &scripts, walker.report, source);
    let Walker {
        lifter,
        inherits,
        requests,
        folders,
        scripts: count,
        ..
    } = walker;
    let (vars, lifted) = with_lifted(vars, lifter);
    for (from, n) in inherits {
        report.info(
            source,
            "",
            format!("{n} request(s) inherit auth from {from}; it was copied into each."),
        );
    }
    Ok(PlannedCollection {
        source: source.to_string(),
        name,
        file,
        lifted,
        vars,
        root,
        requests,
        folders,
        scripts: count,
    })
}

fn collection_file(
    name: &str,
    collection: &schema::Collection,
    scripts: &Scripts,
    report: &mut Report,
    source: &str,
) -> CollectionFile {
    let base = CollectionFile::new(name.to_string());
    let checked = checked_scripts(source, "event", scripts, report);
    let with = base.with_scripts(&checked);
    let mut extra = with_description(
        &disable_scripts(&with.extra),
        collection.info.description.as_deref(),
    );
    if let Some(id) = collection
        .info
        .postman_id
        .as_ref()
        .filter(|id| id.len() <= 64)
    {
        extra.insert(POSTMAN_ID.into(), serde_json::Value::String(id.clone()));
    }
    CollectionFile { extra, ..with }
}

fn with_lifted(vars: PlannedVars, lifter: Lifter) -> (PlannedVars, Vec<String>) {
    let lifted = lifter.into_values();
    let mut next = vars;
    let mut names = Vec::new();
    for (name, value) in lifted.iter() {
        next.table
            .insert(name.to_string(), VarEntry::new(String::new(), true, true));
        next.secrets.insert(name, value);
        names.push(name.to_string());
    }
    (next, names)
}

/// Requests and folders, counted without recursion limits beyond the JSON pre-scan.
fn count_items(items: &[schema::Item]) -> usize {
    items
        .iter()
        .map(|item| match item {
            schema::Item::Folder(folder) => 1 + count_items(&folder.item),
            schema::Item::Request(_) => 1,
        })
        .sum()
}

fn entries(children: &[PlannedNode]) -> Vec<String> {
    children
        .iter()
        .map(|child| match child {
            PlannedNode::Folder(folder) => folder.dir.clone(),
            PlannedNode::Request { entry, .. } => entry.clone(),
        })
        .collect()
}

/// Scripts over the size cap are dropped with a report line.
fn checked_scripts(source: &str, path: &str, scripts: &Scripts, report: &mut Report) -> Scripts {
    match crate::store::script_model::validate_scripts(source, scripts) {
        Ok(()) => scripts.clone(),
        Err(_) => {
            report.skipped(
                source,
                path,
                "A script is larger than 256 KiB and was dropped.",
            );
            Scripts::default()
        }
    }
}

struct Inherited<'a> {
    auth: Option<&'a schema::Auth>,
    from: String,
}

struct Walker<'r> {
    source: &'r str,
    report: &'r mut Report,
    lifter: Lifter,
    inherits: BTreeMap<String, u32>,
    requests: u32,
    folders: u32,
    scripts: u32,
}

impl Walker<'_> {
    fn children(
        &mut self,
        path: &str,
        items: &[schema::Item],
        inherited: &Inherited<'_>,
        depth: usize,
    ) -> Vec<PlannedNode> {
        let mut taken: Vec<String> = Vec::new();
        let mut planned = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let at = format!("{path}[{index}]");
            let node = match item {
                schema::Item::Folder(folder) => self.folder(&at, folder, inherited, depth, &taken),
                schema::Item::Request(request) => self.request(&at, request, inherited, &taken),
            };
            if let Some(node) = node {
                taken.push(entries(std::slice::from_ref(&node)).concat());
                planned.push(node);
            }
        }
        planned
    }

    fn stem(&mut self, at: &str, name: &str, taken: &[String]) -> Option<String> {
        match paths::unique_stem(&paths::slugify(name), taken) {
            Ok(stem) if paths::is_valid_segment(&stem) => Some(stem),
            _ => {
                self.report.skipped(
                    self.source,
                    at,
                    "Too many items share this name; it was skipped.",
                );
                None
            }
        }
    }

    fn name(&mut self, at: &str, raw: &str, fallback: &str) -> String {
        let (name, cut) = clean_display_name(raw, fallback);
        if cut {
            self.report
                .warning(self.source, at, "The name was cut to 100 characters.");
        }
        name
    }

    fn folder(
        &mut self,
        at: &str,
        folder: &schema::Folder,
        inherited: &Inherited<'_>,
        depth: usize,
        taken: &[String],
    ) -> Option<PlannedNode> {
        if depth >= MAX_IMPORT_FOLDER_DEPTH {
            self.report.skipped(
                self.source,
                at,
                format!(
                    "Folders nested deeper than {MAX_IMPORT_FOLDER_DEPTH} levels were skipped."
                ),
            );
            return None;
        }
        let name = self.name(at, &folder.name, UNTITLED_FOLDER);
        let dir = self.stem(at, &name, taken)?;
        let scripts = self.scripts(&format!("{at}.event"), &folder.event);
        let scripts = checked_scripts(self.source, at, &scripts, self.report);
        let inner = Inherited {
            auth: folder.auth.as_ref().or(inherited.auth),
            from: if folder.auth.is_some() {
                format!("folder `{name}`")
            } else {
                inherited.from.clone()
            },
        };
        if !folder.variable.is_empty() {
            self.report.warning(
                self.source,
                at,
                "Folder variables are not supported and were dropped.",
            );
        }
        let children = self.children(&format!("{at}.item"), &folder.item, &inner, depth + 1);
        let base = FolderFile::default()
            .with_name(&name)
            .with_scripts(&scripts)
            .with_order(entries(&children));
        let file = FolderFile {
            extra: with_description(&disable_scripts(&base.extra), folder.description.as_deref()),
            ..base
        };
        self.folders += 1;
        Some(PlannedNode::Folder(PlannedFolder {
            dir,
            file,
            children,
        }))
    }

    fn request(
        &mut self,
        at: &str,
        item: &schema::RequestItem,
        inherited: &Inherited<'_>,
        taken: &[String],
    ) -> Option<PlannedNode> {
        let name = self.name(at, &item.name, UNTITLED_REQUEST);
        let stem = self.stem(at, &name, taken)?;
        let place = At {
            file: self.source,
            path: at,
        };
        let mapped = map_request(place, item, inherited.auth, self.report);
        if item.request.auth.is_none() && inherited.auth.is_some() {
            *self.inherits.entry(inherited.from.clone()).or_default() += 1;
        }
        let (lifted, notes) = self.lifter.lift(&stem, &mapped);
        notes
            .into_iter()
            .for_each(|note| self.report.info(self.source, at, note));
        let content = RequestContent {
            scripts: self.scripts(&format!("{at}.event"), &item.event),
            ..lifted
        };
        match request_file(&name, &content, item.request.description.as_deref()) {
            Ok(file) => {
                self.requests += 1;
                Some(PlannedNode::Request {
                    entry: format!("{stem}{REQUEST_SUFFIX}"),
                    file,
                })
            }
            Err(error) => {
                let problem = error
                    .message
                    .strip_prefix("Request: ")
                    .unwrap_or(&error.message);
                self.report.skipped(
                    self.source,
                    at,
                    format!("The request was skipped: {problem}"),
                );
                None
            }
        }
    }

    /// Scripts of `events`, verbatim, counted and scanned.
    fn scripts(&mut self, path: &str, events: &[schema::Event]) -> Scripts {
        let (scripts, count) = plan_scripts(self.source, path, events, self.report);
        self.scripts += count;
        scripts
    }
}

#[cfg(test)]
#[path = "tree_tests.rs"]
mod tests;
