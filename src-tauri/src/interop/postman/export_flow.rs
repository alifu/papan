//! Export flow without Tauri (plan 09 §7): our collection (plus chosen environments and
//! globals) → Postman JSON files in memory, then written to a picked folder with names that
//! never overwrite. Secret values come from the vault only when the user asked for them.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::export::{
    environment, export_collection, globals, to_json, SecretValues, COLLECTION_SUFFIX,
    ENVIRONMENT_SUFFIX,
};
use super::flow::cap_items;
use super::types::{ExportOptions, PostmanExportResult, ReportItem};
use crate::error::AppError;
use crate::store::env_model::VarTable;
use crate::store::fsio;
use crate::store::paths;
use crate::store::Store;
use crate::vars::service::{collection_prefix, env_prefix, global_prefix};
use crate::vars::vault::SecretStore;

const MAX_COPY: u32 = 999;
const GLOBALS_STEM: &str = "globals";
const GLOBALS_SUFFIX: &str = ".postman_globals.json";

/// Rendered files (`stem`, `suffix`, JSON) and the report, before any disk write.
pub struct ExportBundle {
    files: Vec<(String, String, String)>,
    report: Vec<ReportItem>,
}

/// File names only: the JSON may hold secret values when the user asked for them.
impl std::fmt::Debug for ExportBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<String> = self
            .files
            .iter()
            .map(|(stem, suffix, _)| format!("{stem}{suffix}"))
            .collect();
        f.debug_struct("ExportBundle")
            .field("files", &names)
            .finish()
    }
}

impl ExportBundle {
    pub fn file_count(&self) -> usize {
        self.files.len()
    }
}

/// A stable v4-shaped id from `parts` (FNV-1a, 128 bit): equal input, equal bytes. Used
/// when nothing was imported, so exports of one collection never change between runs.
fn stable_id(parts: &[&str]) -> String {
    const OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
    let hash = parts
        .iter()
        .flat_map(|part| part.bytes().chain(std::iter::once(0)))
        .fold(OFFSET, |hash, byte| {
            (hash ^ u128::from(byte)).wrapping_mul(PRIME)
        });
    uuid::Builder::from_random_bytes(hash.to_be_bytes())
        .into_uuid()
        .to_string()
}

/// Reads the collection, environments and globals from disk (initial values only).
pub fn build_export(
    store: &Store,
    vault: &dyn SecretStore,
    options: &ExportOptions,
) -> Result<ExportBundle, AppError> {
    let id = &options.collection_id;
    let collection = store.collection(id)?;
    let secrets = |prefix: String, table: &VarTable| -> Result<Option<SecretValues>, AppError> {
        options
            .include_secret_values
            .then(|| vault_values(vault, &prefix, table))
            .transpose()
    };
    let mut files = Vec::new();
    let table = store.collection_variables(id)?;
    let collection_name = collection.read_collection_file()?.name;
    let exported = export_collection(
        &collection,
        &stable_id(&["collection", &collection_name]),
        secrets(collection_prefix(id), &table)?.as_ref(),
    )?;
    let stem = exported
        .file_name
        .strip_suffix(COLLECTION_SUFFIX)
        .unwrap_or(&exported.file_name)
        .to_string();
    files.push((stem, COLLECTION_SUFFIX.to_string(), exported.json));
    let report = exported.report;
    let mut taken = Vec::new();
    let mut chosen = BTreeSet::new();
    for env in options.environments.iter().filter(|e| chosen.insert(*e)) {
        let file = store.read_environment(id, env)?;
        let values = secrets(env_prefix(id, env), &file.variables)?;
        let json = to_json(&environment(
            &file,
            &stable_id(&["environment", &collection_name, &file.name]),
            values.as_ref(),
        ))?;
        let stem = paths::unique_stem(&paths::slugify(&file.name), &taken)?;
        taken.push(stem.clone());
        files.push((stem, ENVIRONMENT_SUFFIX.to_string(), json));
    }
    if options.include_globals {
        let file = store.globals()?;
        let values = secrets(global_prefix(), &file.variables)?;
        let json = to_json(&globals(&file, values.as_ref()))?;
        files.push((GLOBALS_STEM.to_string(), GLOBALS_SUFFIX.to_string(), json));
    }
    Ok(ExportBundle { files, report })
}

fn vault_values(
    vault: &dyn SecretStore,
    prefix: &str,
    table: &VarTable,
) -> Result<SecretValues, AppError> {
    let mut values = SecretValues::new();
    for (name, _) in table.iter().filter(|(_, entry)| entry.secret) {
        if let Some(value) = vault.get(&format!("{prefix}{name}"))? {
            values.insert(name.clone(), value);
        }
    }
    Ok(values)
}

/// Writes every file into `dir` (an existing folder); a name already there gets ` (2)`,
/// ` (3)`… Nothing is overwritten; if a write fails the files of this run are removed.
pub fn write_bundle(dir: &Path, bundle: ExportBundle) -> Result<PostmanExportResult, AppError> {
    if fsio::entry_kind(dir) != Some(fsio::EntryKind::Dir) {
        return Err(AppError::not_found("That folder does not exist."));
    }
    let mut written: Vec<(String, PathBuf)> = Vec::new();
    for (stem, suffix, json) in &bundle.files {
        let name = free_name(dir, stem, suffix, &written)?;
        let path = dir.join(&name);
        if let Err(error) = fsio::write_atomic(&path, json.as_bytes()) {
            for (_, done) in &written {
                if let Err(cleanup) = fsio::remove_file(done) {
                    tracing::warn!(detail = ?cleanup.detail, "could not remove a partial export file");
                }
            }
            return Err(error);
        }
        written.push((name, path));
    }
    Ok(PostmanExportResult {
        files: written.into_iter().map(|(name, _)| name).collect(),
        report: cap_items(bundle.report),
    })
}

fn free_name(
    dir: &Path,
    stem: &str,
    suffix: &str,
    written: &[(String, PathBuf)],
) -> Result<String, AppError> {
    let mut candidate = format!("{stem}{suffix}");
    let mut copy = 2;
    while fsio::entry_kind(&dir.join(&candidate)).is_some()
        || written.iter().any(|(name, _)| *name == candidate)
    {
        if copy > MAX_COPY {
            return Err(AppError::invalid_input(
                "That folder has too many exports with this name.",
            ));
        }
        candidate = format!("{stem} ({copy}){suffix}");
        copy += 1;
    }
    Ok(candidate)
}
