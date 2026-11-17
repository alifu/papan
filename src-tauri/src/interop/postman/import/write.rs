//! Puts a chosen plan on disk (plan 09 §4 "Writing"): everything is rendered and validated
//! in memory, written into a fresh staging directory (every path through `paths::resolve`),
//! then moved into place with one `rename`. Any failure removes what was made: nothing
//! partial. Secret values go to the vault only afterwards, via `vault::apply_secrets`, and only
//! when the user opted in.

use std::path::{Path, PathBuf};

use super::rename::rename_clashing;
use super::render::{env_file_name, render_as_folder, render_collection, render_env, Output};
use super::secrets::SecretValues;
use super::vars::PlannedVars;
use super::{ImportPlan, PlannedEnv};
use crate::error::AppError;
use crate::interop::postman::limits::{MAX_ENVIRONMENTS, MAX_VARIABLES};
use crate::interop::postman::report::Report;
use crate::interop::postman::types::{GlobalsClash, ImportOptions};
use crate::store::collection::Collection;
use crate::store::env_model::VarTable;
use crate::store::fsio::{self, EntryKind};
use crate::store::paths;
use crate::store::Store;

const STAGING_PREFIX: &str = ".papan-import-";
const GLOBALS: &str = "globals";

/// Secret values waiting for the vault (empty unless the user opted in).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PendingSecrets {
    pub collection: SecretValues,
    /// By environment stem.
    pub environments: Vec<(String, SecretValues)>,
    pub globals: SecretValues,
}

/// What an import wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct Written {
    /// The new collection's root, or the open collection's root.
    pub root: PathBuf,
    /// The new top-level folder (relative to `requests/`) of an import into a collection.
    pub folder: Option<String>,
    /// Environment stems written.
    pub environments: Vec<String>,
    pub globals: u32,
    pub pending: PendingSecrets,
    pub report: Report,
}

/// A new collection in `parent` (dir `unique_stem(slugify(name))`), plus globals.
pub fn write_new_collection(
    plan: &ImportPlan,
    options: &ImportOptions,
    parent: &Path,
    store: &Store,
) -> Result<Written, AppError> {
    let collection = plan.collection.as_ref().ok_or_else(|| {
        AppError::invalid_input("Pick a Postman collection to create a new collection.")
    })?;
    let mut report = Report::new();
    if fsio::entry_kind(parent) != Some(EntryKind::Dir) {
        return Err(AppError::not_found("That folder does not exist."));
    }
    let envs = env_stems(&plan.environments, Vec::new(), 0, &mut report);
    let outputs = render_collection(collection, collection.vars.table.clone(), &envs)?;
    let name = paths::unique_stem(&paths::slugify(&collection.name), &names_in(parent)?)?;
    let root = parent.join(&name);
    stage_and_rename(parent, &outputs, &root)?;
    let globals = write_globals(plan, options, store, &mut report)
        .map_err(|error| undo(std::slice::from_ref(&root), error))?;
    let pending = pending(options, &envs, collection.vars.secrets.clone(), globals.1);
    Ok(Written {
        root,
        folder: None,
        environments: envs.into_iter().map(|(stem, _)| stem).collect(),
        globals: globals.0,
        pending,
        report,
    })
}

/// A new top-level folder in the open collection `id`, its environments and globals.
/// Collection variables are added where the name is free (existing ones win).
pub fn write_into_collection(
    plan: &ImportPlan,
    options: &ImportOptions,
    store: &mut Store,
    id: &str,
) -> Result<Written, AppError> {
    let target = store.collection(id)?;
    let mut report = Report::new();
    let mut made: Vec<PathBuf> = Vec::new();
    let previous = store.collection_variables(id)?;
    let collection = match &plan.collection {
        Some(collection) => {
            let (renamed, renames) = rename_clashing(collection, &previous)?;
            for (old, new) in &renames {
                report.warning(
                    "",
                    "",
                    format!("The credential variable `{old}` already exists here; the imported one is `{new}`."),
                );
            }
            Some(renamed)
        }
        None => None,
    };
    let folder = match &collection {
        Some(collection) => Some(write_folder(&target, collection, &mut made)?),
        None => None,
    };
    let envs = match write_envs(&target, &plan.environments, &mut report, &mut made) {
        Ok(envs) => envs,
        Err(error) => return Err(undo(&made, error)),
    };
    let added = match &collection {
        Some(collection) => merge_collection_vars(store, id, &collection.vars, &mut report)
            .map_err(|error| undo(&made, error))?,
        None => SecretValues::default(),
    };
    let globals = match write_globals(plan, options, store, &mut report) {
        Ok(globals) => globals,
        Err(error) => {
            if plan.collection.is_some() {
                if let Err(restore) = store.write_collection_variables(id, previous) {
                    tracing::warn!(detail = ?restore.detail, "could not restore collection variables");
                }
            }
            return Err(undo(&made, error));
        }
    };
    Ok(Written {
        root: target.root.clone(),
        folder,
        environments: envs.iter().map(|(stem, _)| stem.clone()).collect(),
        globals: globals.0,
        pending: pending(options, &envs, added, globals.1),
        report,
    })
}

fn pending(
    options: &ImportOptions,
    envs: &[(String, &PlannedEnv)],
    collection: SecretValues,
    globals: SecretValues,
) -> PendingSecrets {
    if !options.store_secret_values {
        return PendingSecrets::default();
    }
    PendingSecrets {
        collection,
        environments: envs
            .iter()
            .map(|(stem, env)| (stem.clone(), env.vars.secrets.clone()))
            .collect(),
        globals,
    }
}

fn names_in(dir: &Path) -> Result<Vec<String>, AppError> {
    Ok(fsio::list_dir(dir)?
        .into_iter()
        .map(|entry| entry.name)
        .collect())
}

/// Each environment that fits with its unique stem among `taken` names, while `used`
/// environments already exist; envs over the cap are reported and left out.
fn env_stems<'a>(
    envs: &'a [PlannedEnv],
    taken: Vec<String>,
    used: usize,
    report: &mut Report,
) -> Vec<(String, &'a PlannedEnv)> {
    let room = MAX_ENVIRONMENTS.saturating_sub(used);
    let mut taken = taken;
    let mut kept: Vec<(String, &PlannedEnv)> = Vec::new();
    for env in envs {
        let stem = paths::unique_stem(&paths::slugify(&env.name), &taken);
        match stem {
            Ok(stem) if kept.len() < room => {
                taken.push(stem.clone());
                kept.push((stem, env));
            }
            _ => report.skipped(
                &env.source,
                "",
                "The collection has no room for this environment.",
            ),
        }
    }
    kept
}

/// Writes `outputs` into a fresh staging dir in `parent`, then renames it to `target`.
fn stage_and_rename(parent: &Path, outputs: &[Output], target: &Path) -> Result<(), AppError> {
    if fsio::entry_kind(target).is_some() {
        return Err(AppError::invalid_input(
            "A folder with that name already exists there.",
        ));
    }
    let staging = parent.join(format!("{STAGING_PREFIX}{}", uuid::Uuid::new_v4()));
    fsio::create_dir(&staging)?;
    let done = write_outputs(&staging, outputs).and_then(|()| fsio::rename(&staging, target));
    if done.is_err() {
        if let Err(error) = fsio::remove_dir_all(&staging) {
            tracing::warn!(detail = ?error.detail, "could not remove an import staging folder");
        }
    }
    done
}

fn write_outputs(staging: &Path, outputs: &[Output]) -> Result<(), AppError> {
    for output in outputs {
        match output {
            Output::Dir(rel) => {
                let path = paths::resolve(staging, rel)?;
                fsio::create_dir(&path)?;
            }
            Output::File(rel, bytes) => {
                let path = paths::resolve(staging, rel)?;
                if fsio::entry_kind(&path).is_some() {
                    return Err(AppError::internal("An import wrote one file twice."));
                }
                fsio::write_atomic(&path, bytes)?;
            }
        }
    }
    Ok(())
}

fn write_folder(
    target: &Collection,
    collection: &super::tree::PlannedCollection,
    made: &mut Vec<PathBuf>,
) -> Result<String, AppError> {
    let requests = target.requests_root();
    if fsio::entry_kind(&requests).is_none() {
        fsio::create_dir(&requests)?;
    }
    let outputs = render_as_folder(collection)?;
    let stem = paths::unique_stem(&paths::slugify(&collection.name), &names_in(&requests)?)?;
    let folder = requests.join(&stem);
    stage_and_rename(&requests, &outputs, &folder)?;
    made.push(folder);
    Ok(stem)
}

fn write_envs<'a>(
    target: &Collection,
    envs: &'a [PlannedEnv],
    report: &mut Report,
    made: &mut Vec<PathBuf>,
) -> Result<Vec<(String, &'a PlannedEnv)>, AppError> {
    if envs.is_empty() {
        return Ok(Vec::new());
    }
    let dir = target.environments_dir();
    if fsio::entry_kind(&dir).is_none() {
        fsio::create_dir(&dir)?;
        made.push(dir.clone());
    }
    let stems = env_stems(envs, names_in(&dir)?, target.env_stems().len(), report);
    let outputs: Vec<Output> = stems
        .iter()
        .map(|(stem, env)| Ok(Output::File(env_file_name(stem), render_env(env)?)))
        .collect::<Result<_, AppError>>()?;
    let staging = target
        .root
        .join(format!("{STAGING_PREFIX}{}", uuid::Uuid::new_v4()));
    fsio::create_dir(&staging)?;
    let result = write_outputs(&staging, &outputs).and_then(|()| {
        stems.iter().try_for_each(|(stem, _)| {
            let name = env_file_name(stem);
            let to = dir.join(&name);
            if fsio::entry_kind(&to).is_some() {
                return Err(AppError::invalid_input(
                    "An environment with that name already exists.",
                ));
            }
            fsio::rename(&staging.join(&name), &to)?;
            made.push(to);
            Ok(())
        })
    });
    if let Err(error) = fsio::remove_dir_all(&staging) {
        tracing::warn!(detail = ?error.detail, "could not remove an import staging folder");
    }
    result.map(|()| stems)
}

/// Adds the imported collection variables whose names are free; returns their secrets.
fn merge_collection_vars(
    store: &mut Store,
    id: &str,
    vars: &PlannedVars,
    report: &mut Report,
) -> Result<SecretValues, AppError> {
    let existing = store.collection_variables(id)?;
    let (merged, secrets) = merge(&existing, vars, GlobalsClash::Keep, "variable", report);
    if merged != existing {
        store.write_collection_variables(id, merged)?;
    }
    Ok(secrets)
}

/// `existing` plus `vars` under `clash`, capped at 500; also the secrets that were written.
fn merge(
    existing: &VarTable,
    vars: &PlannedVars,
    clash: GlobalsClash,
    what: &str,
    report: &mut Report,
) -> (VarTable, SecretValues) {
    let mut merged = existing.clone();
    let mut secrets = SecretValues::default();
    for (name, entry) in &vars.table {
        let present = merged.contains_key(name);
        if present && clash == GlobalsClash::Keep {
            report.skipped(
                "",
                "",
                format!("The {what} `{name}` already exists and was kept."),
            );
            continue;
        }
        if !present && merged.len() >= MAX_VARIABLES {
            report.skipped(
                "",
                "",
                format!("No room for the {what} `{name}` (at most 500)."),
            );
            continue;
        }
        if let Some(value) = vars.secrets.get(name) {
            secrets.insert(name, value);
        }
        merged.insert(name.clone(), entry.clone());
    }
    (merged, secrets)
}

/// Merges the plan's globals into the stored ones.
fn write_globals(
    plan: &ImportPlan,
    options: &ImportOptions,
    store: &Store,
    report: &mut Report,
) -> Result<(u32, SecretValues), AppError> {
    let Some(globals) = &plan.globals else {
        return Ok((0, SecretValues::default()));
    };
    store.globals().and_then(|current| {
        let (merged, secrets) = merge(
            &current.variables,
            globals,
            options.globals_clash,
            GLOBALS,
            report,
        );
        let count = changed_count(&current.variables, &merged);
        store.write_globals(merged).map(|()| (count, secrets))
    })
}

fn changed_count(before: &VarTable, after: &VarTable) -> u32 {
    let changed = after
        .iter()
        .filter(|(name, entry)| before.get(*name) != Some(*entry))
        .count();
    u32::try_from(changed).unwrap_or(u32::MAX)
}

/// Removes what this import made (newest first) and returns `error`.
fn undo(made: &[PathBuf], error: AppError) -> AppError {
    for path in made.iter().rev() {
        let removed = match fsio::entry_kind(path) {
            Some(EntryKind::Dir) => fsio::remove_dir_all(path),
            Some(_) => fsio::remove_file(path),
            None => Ok(()),
        };
        if let Err(problem) = removed {
            tracing::warn!(detail = ?problem.detail, "could not undo part of an import");
        }
    }
    error
}

#[cfg(test)]
#[path = "write_tests.rs"]
mod tests;
