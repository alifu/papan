//! A whole collection on disk → one v2.1 collection file. Walks the tree in display order;
//! items that cannot be read are skipped and reported, never fatal.

use std::collections::BTreeMap;

use super::request::{events, request_item};
use super::vars::{collection_variables, POSTMAN_ID};
use super::{to_json, Report, SecretValues, COLLECTION_SUFFIX};
use crate::error::AppError;
use crate::interop::postman::schema::{Collection as PmCollection, Folder, Info, Item, SCHEMA_V21};
use crate::interop::postman::types::{ReportItem, ReportLevel};
use crate::store::collection::{Collection, COLLECTION_FILE};
use crate::store::fsio::{self, EntryKind};
use crate::store::model::{FolderFile, RequestFile};
use crate::store::paths::{self, FOLDER_FILE};
use crate::store::types::{NodeKind, TreeNode};

const DESCRIPTION: &str = "description";
const SECRET_AS_STRING: &str =
    "Postman collection variables cannot be secret; exported as a plain variable (empty unless secret values are included).";

const SECRET_VALUE_INCLUDED: &str =
    "The secret value is in the file; importing it again will not mark it secret unless its name looks sensitive.";

#[derive(Debug, Clone, PartialEq)]
pub struct ExportedCollection {
    /// `<slug>.postman_collection.json` (the caller makes it unique in the target folder).
    pub file_name: String,
    pub json: String,
    pub report: Vec<ReportItem>,
}

/// `fallback_id` is used when the collection has no imported `postman_id` (callers pass a v4).
pub fn export_collection(
    collection: &Collection,
    fallback_id: &str,
    secrets: Option<&SecretValues>,
) -> Result<ExportedCollection, AppError> {
    let file = collection.read_collection_file()?;
    let file_name = format!(
        "{}{COLLECTION_SUFFIX}",
        paths::unique_stem(&paths::slugify(&file.name), &[])?
    );
    let mut report = Report::new(&file_name);
    let info = Info {
        postman_id: Some(text(&file.extra, POSTMAN_ID).unwrap_or_else(|| fallback_id.to_string())),
        name: file.name.clone(),
        description: text(&file.extra, DESCRIPTION),
        schema: SCHEMA_V21.to_string(),
    };
    for name in file
        .variables
        .iter()
        .filter(|(_, v)| v.secret)
        .map(|(k, _)| k)
    {
        report.add(
            ReportLevel::Info,
            &format!("variable.{name}"),
            SECRET_AS_STRING,
        );
        if secrets.is_some_and(|values| values.contains_key(name)) {
            report.add(
                ReportLevel::Warning,
                &format!("variable.{name}"),
                SECRET_VALUE_INCLUDED,
            );
        }
    }
    let pm = PmCollection {
        info,
        item: items(collection, &collection.tree(), &mut report),
        auth: None,
        event: events(&file.scripts(COLLECTION_FILE)?),
        variable: collection_variables(&file.variables, secrets),
        protocol_profile_behavior: None,
    };
    Ok(ExportedCollection {
        json: to_json(&pm)?,
        file_name,
        report: report.into_items(),
    })
}

fn items(collection: &Collection, nodes: &[TreeNode], report: &mut Report) -> Vec<Item> {
    nodes
        .iter()
        .filter_map(|node| item(collection, node, report))
        .collect()
}

fn item(collection: &Collection, node: &TreeNode, report: &mut Report) -> Option<Item> {
    let read = match node.kind {
        NodeKind::Folder => folder(collection, node, report).map(|f| Some(Item::Folder(f))),
        NodeKind::Request => request(collection, node, report),
        NodeKind::Invalid => Err(AppError::invalid_input("This item could not be read.")),
    };
    match read {
        Ok(item) => item,
        Err(error) => {
            report.add(
                ReportLevel::Skipped,
                &node.path,
                format!("{} Skipped.", error.message),
            );
            None
        }
    }
}

fn folder(
    collection: &Collection,
    node: &TreeNode,
    report: &mut Report,
) -> Result<Folder, AppError> {
    let file = read_folder(collection, &node.path)?;
    Ok(Folder {
        name: node.name.clone(),
        description: text(&file.extra, DESCRIPTION),
        item: items(collection, &node.children, report),
        auth: None,
        event: events(&file.scripts(FOLDER_FILE)?),
        variable: Vec::new(),
    })
}

fn request(
    collection: &Collection,
    node: &TreeNode,
    report: &mut Report,
) -> Result<Option<Item>, AppError> {
    let file: RequestFile = collection.read_request(&node.path)?;
    let content = file.content()?;
    let description = text(&file.extra, DESCRIPTION);
    let item = request_item(&file.name, &content, description, &node.path, report);
    Ok(item.map(|r| Item::Request(Box::new(r))))
}

fn read_folder(collection: &Collection, rel: &str) -> Result<FolderFile, AppError> {
    let path = paths::resolve(&collection.requests_root(), rel)?.join(FOLDER_FILE);
    match fsio::entry_kind(&path) {
        None => Ok(FolderFile::default()),
        Some(EntryKind::File) => FolderFile::parse(FOLDER_FILE, &fsio::read_bounded(&path)?),
        Some(_) => Err(AppError::invalid_input("That path is not allowed.")),
    }
}

fn text(extra: &BTreeMap<String, serde_json::Value>, key: &str) -> Option<String> {
    extra.get(key).and_then(|v| v.as_str()).map(str::to_string)
}
