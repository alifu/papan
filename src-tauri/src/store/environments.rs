//! `environments/` of a collection (plan 04 §3.2). The file stem is an environment's
//! identity; renaming changes only `name:`. Every webview-supplied `env` goes through
//! [`env_path`].

use std::path::PathBuf;

use super::collection::{Collection, COLLECTION_FILE};
use super::env_model::{EnvFile, VarTable};
use super::fsio::{self, EntryKind};
use super::model::{self, CollectionFile};
use super::paths;
use crate::error::AppError;
use crate::vars::types::EnvironmentSummary;

pub const ENVIRONMENTS_DIR: &str = "environments";
pub const ENV_SUFFIX: &str = ".env.yaml";
pub const MAX_ENVIRONMENTS: usize = 50;

fn not_allowed(env: &str) -> AppError {
    AppError::invalid_input("That environment name is not allowed.").with_detail(format!("{env:?}"))
}

fn env_missing(env: &str) -> AppError {
    AppError::not_found("That environment does not exist.").with_detail(format!("{env:?}"))
}

fn file_name(env: &str) -> String {
    format!("{env}{ENV_SUFFIX}")
}

impl Collection {
    pub fn environments_dir(&self) -> PathBuf {
        self.root.join(ENVIRONMENTS_DIR)
    }

    /// Stems of the regular `*.env.yaml` files (no symlinks), sorted, at most 50.
    pub fn env_stems(&self) -> Vec<String> {
        let dir = self.environments_dir();
        if fsio::entry_kind(&dir) != Some(EntryKind::Dir) {
            return Vec::new();
        }
        let mut stems: Vec<String> = fsio::list_dir(&dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|entry| entry.kind == EntryKind::File)
            .filter_map(|entry| entry.name.strip_suffix(ENV_SUFFIX).map(str::to_string))
            .filter(|stem| is_valid_env(stem))
            .collect();
        stems.sort();
        stems.truncate(MAX_ENVIRONMENTS);
        stems
    }

    /// The path of environment `env`, checked to stay inside `environments/`.
    pub fn env_path(&self, env: &str) -> Result<PathBuf, AppError> {
        if !is_valid_env(env) {
            return Err(not_allowed(env));
        }
        let dir = self.environments_dir();
        if fsio::entry_kind(&dir).is_some_and(|kind| kind != EntryKind::Dir) {
            return Err(not_allowed(env));
        }
        let path = dir.join(file_name(env));
        match fsio::entry_kind(&path) {
            None => {}
            Some(EntryKind::File) if listed_exactly(&dir, &file_name(env)) => {}
            // Another kind, or a case-insensitive file system resolving `DEV` to `dev`.
            Some(_) => return Err(not_allowed(env)),
        }
        if fsio::entry_kind(&dir) == Some(EntryKind::Dir) {
            let canonical_root = fsio::canonicalize(&self.root)?;
            if !fsio::canonicalize(&dir)?.starts_with(&canonical_root) {
                return Err(not_allowed(env));
            }
        }
        Ok(path)
    }

    /// Environments sorted by name (ignoring case), then `env`; a file that fails to parse is `invalid`.
    pub fn environments(&self) -> Vec<EnvironmentSummary> {
        let mut listed: Vec<EnvironmentSummary> = self
            .env_stems()
            .into_iter()
            .map(|env| match self.read_env(&env) {
                Ok(file) => EnvironmentSummary {
                    env,
                    name: file.name,
                    invalid: false,
                },
                Err(error) => {
                    tracing::warn!(detail = ?error.detail, "{error}");
                    EnvironmentSummary {
                        name: env.clone(),
                        env,
                        invalid: true,
                    }
                }
            })
            .collect();
        listed.sort_by_cached_key(|e| (e.name.to_lowercase(), e.env.clone()));
        listed
    }

    pub fn read_env(&self, env: &str) -> Result<EnvFile, AppError> {
        let path = self.env_path(env)?;
        if fsio::entry_kind(&path) != Some(EntryKind::File) {
            return Err(env_missing(env));
        }
        EnvFile::parse(&file_name(env), &fsio::read_bounded(&path)?)
    }

    fn write_env(&self, env: &str, file: &EnvFile) -> Result<(), AppError> {
        let path = self.env_path(env)?;
        fsio::write_atomic(&path, file.to_yaml()?.as_bytes())?;
        Ok(())
    }

    /// Creates an environment named `name`; returns its stem.
    pub fn create_env(&self, name: &str) -> Result<String, AppError> {
        let name = model::clean_name(name)?;
        let existing = self.env_stems();
        if existing.len() >= MAX_ENVIRONMENTS {
            return Err(AppError::invalid_input(
                "A collection can have at most 50 environments.",
            ));
        }
        let dir = self.environments_dir();
        if fsio::entry_kind(&dir).is_none() {
            fsio::create_dir(&dir)?;
        }
        let siblings: Vec<String> = fsio::list_dir(&dir)
            .unwrap_or_default()
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        let env = paths::unique_stem(&paths::slugify(&name), &siblings)?;
        self.write_env(&env, &EnvFile::new(name))?;
        Ok(env)
    }

    pub fn rename_env(&self, env: &str, name: &str) -> Result<(), AppError> {
        let name = model::clean_name(name)?;
        let file = self.read_env(env)?;
        self.write_env(env, &EnvFile { name, ..file })
    }

    /// Deletes the file, also when it is invalid.
    pub fn delete_env(&self, env: &str) -> Result<(), AppError> {
        let path = self.env_path(env)?;
        if fsio::entry_kind(&path) != Some(EntryKind::File) {
            return Err(env_missing(env));
        }
        fsio::remove_file(&path)
    }

    pub fn write_env_variables(&self, env: &str, variables: VarTable) -> Result<(), AppError> {
        let file = self.read_env(env)?;
        self.write_env(env, &EnvFile { variables, ..file })
    }

    pub fn read_collection_file(&self) -> Result<CollectionFile, AppError> {
        let bytes = fsio::read_bounded(&self.root.join(COLLECTION_FILE))?;
        CollectionFile::parse(COLLECTION_FILE, &bytes)
    }

    pub fn write_collection_variables(&self, variables: VarTable) -> Result<(), AppError> {
        let file = self.read_collection_file()?.with_variables(variables);
        fsio::write_atomic(&self.root.join(COLLECTION_FILE), file.to_yaml()?.as_bytes())?;
        Ok(())
    }
}

/// Whether `dir` holds an entry named exactly `name` (case-sensitive).
fn listed_exactly(dir: &std::path::Path, name: &str) -> bool {
    fsio::list_dir(dir)
        .unwrap_or_default()
        .iter()
        .any(|entry| entry.name == name)
}

/// An `env` value from the webview: one safe path segment with no `.`.
pub fn is_valid_env(env: &str) -> bool {
    paths::is_valid_segment(env) && !env.contains(['/', '.'])
}
