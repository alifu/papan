//! A plan rendered to the exact directories and file bytes to write, each parsed back
//! with the store's own parser first, so nothing invalid ever reaches the disk.

use super::tree::{PlannedCollection, PlannedFolder, PlannedNode};
use super::PlannedEnv;
use crate::error::AppError;
use crate::store::collection::{COLLECTION_FILE, REQUESTS_DIR};
use crate::store::env_model::{EnvFile, VarTable};
use crate::store::environments::{ENVIRONMENTS_DIR, ENV_SUFFIX};
use crate::store::model::{CollectionFile, FolderFile, RequestFile};
use crate::store::paths::{self, FOLDER_FILE};

/// One thing to create, relative to the staging directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    Dir(String),
    File(String, Vec<u8>),
}

/// A whole new collection: collection file, `requests/` tree and `environments/`.
pub fn render_collection(
    collection: &PlannedCollection,
    variables: VarTable,
    environments: &[(String, &PlannedEnv)],
) -> Result<Vec<Output>, AppError> {
    let file = collection.file.with_variables(variables);
    let text = file.to_yaml()?;
    CollectionFile::parse(COLLECTION_FILE, text.as_bytes())?;
    let mut outputs = vec![
        Output::File(COLLECTION_FILE.into(), text.into_bytes()),
        Output::Dir(REQUESTS_DIR.into()),
    ];
    render_folder(REQUESTS_DIR, &collection.root, &mut outputs)?;
    if !environments.is_empty() {
        outputs.push(Output::Dir(ENVIRONMENTS_DIR.into()));
        for (stem, env) in environments {
            let rel = paths::join_rel(ENVIRONMENTS_DIR, &env_file_name(stem));
            outputs.push(Output::File(rel, render_env(env)?));
        }
    }
    Ok(outputs)
}

/// The collection as one folder (for an import into an open collection), relative to the
/// folder itself: its `_folder.yaml` carries the name, description and scripts.
pub fn render_as_folder(collection: &PlannedCollection) -> Result<Vec<Output>, AppError> {
    // The Postman id belongs to a whole collection, not to a folder.
    let extra = collection
        .file
        .extra
        .iter()
        .filter(|(key, _)| key.as_str() != "postman_id")
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let file = FolderFile {
        extra,
        ..collection.root.file.with_name(&collection.name)
    };
    let root = PlannedFolder {
        file,
        ..collection.root.clone()
    };
    let mut outputs = Vec::new();
    render_folder("", &root, &mut outputs)?;
    Ok(outputs)
}

pub fn env_file_name(stem: &str) -> String {
    format!("{stem}{ENV_SUFFIX}")
}

pub fn render_env(env: &PlannedEnv) -> Result<Vec<u8>, AppError> {
    let file = EnvFile {
        variables: env.vars.table.clone(),
        ..EnvFile::new(env.name.clone())
    };
    let text = file.to_yaml()?;
    EnvFile::parse(&env.source, text.as_bytes())?;
    Ok(text.into_bytes())
}

fn render_folder(
    rel: &str,
    folder: &PlannedFolder,
    outputs: &mut Vec<Output>,
) -> Result<(), AppError> {
    if !folder.file.is_empty() {
        let text = folder.file.to_yaml()?;
        FolderFile::parse(FOLDER_FILE, text.as_bytes())?;
        outputs.push(Output::File(
            paths::join_rel(rel, FOLDER_FILE),
            text.into_bytes(),
        ));
    }
    for child in &folder.children {
        match child {
            PlannedNode::Folder(inner) => {
                let child_rel = segment(rel, &inner.dir)?;
                outputs.push(Output::Dir(child_rel.clone()));
                render_folder(&child_rel, inner, outputs)?;
            }
            PlannedNode::Request { entry, file } => {
                let child_rel = segment(rel, entry)?;
                let text = file.to_yaml()?;
                RequestFile::parse(entry, text.as_bytes())?;
                outputs.push(Output::File(child_rel, text.into_bytes()));
            }
        }
    }
    Ok(())
}

/// `rel/name`, refusing any name that is not one safe path segment.
fn segment(rel: &str, name: &str) -> Result<String, AppError> {
    if !paths::is_valid_segment(name) || name == FOLDER_FILE {
        return Err(AppError::internal("An import produced an unsafe name.")
            .with_detail(format!("{name:?}")));
    }
    Ok(paths::join_rel(rel, name))
}
