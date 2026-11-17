//! Store operations on environments and variable files (plan 04 §3). Kept apart from
//! `service.rs`. Every write refreshes the fingerprint, so our own writes fire no
//! `CollectionChanged`.

use std::path::PathBuf;

use super::collection::Collection;
use super::env_model::{EnvFile, GlobalsFile, VarTable};
use super::fsio::{self, EntryKind};
use super::Store;
use crate::error::AppError;
use crate::vars::types::EnvironmentSummary;

pub const GLOBALS_FILE: &str = "globals.yaml";

impl Store {
    /// The open collection `id`.
    pub fn collection(&self, id: &str) -> Result<Collection, AppError> {
        self.open_entry(id).map(|(_, collection)| collection)
    }

    fn change<T>(
        &mut self,
        id: &str,
        operation: impl FnOnce(&Collection) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let collection = self.collection(id)?;
        let result = operation(&collection);
        self.remember(id, &collection);
        result
    }

    pub fn environments(&self, id: &str) -> Result<Vec<EnvironmentSummary>, AppError> {
        Ok(self.collection(id)?.environments())
    }

    pub fn create_environment(&mut self, id: &str, name: &str) -> Result<String, AppError> {
        self.change(id, |c| c.create_env(name))
    }

    pub fn rename_environment(&mut self, id: &str, env: &str, name: &str) -> Result<(), AppError> {
        self.change(id, |c| c.rename_env(env, name))
    }

    pub fn delete_environment(&mut self, id: &str, env: &str) -> Result<(), AppError> {
        self.change(id, |c| c.delete_env(env))
    }

    pub fn read_environment(&self, id: &str, env: &str) -> Result<EnvFile, AppError> {
        self.collection(id)?.read_env(env)
    }

    pub fn write_environment_variables(
        &mut self,
        id: &str,
        env: &str,
        variables: VarTable,
    ) -> Result<(), AppError> {
        self.change(id, |c| c.write_env_variables(env, variables))
    }

    pub fn collection_variables(&self, id: &str) -> Result<VarTable, AppError> {
        Ok(self.collection(id)?.read_collection_file()?.variables)
    }

    pub fn write_collection_variables(
        &mut self,
        id: &str,
        variables: VarTable,
    ) -> Result<(), AppError> {
        self.change(id, |c| c.write_collection_variables(variables))
    }

    fn globals_path(&self) -> PathBuf {
        self.app_data().join(GLOBALS_FILE)
    }

    /// Globals; a missing file is empty.
    pub fn globals(&self) -> Result<GlobalsFile, AppError> {
        let path = self.globals_path();
        if fsio::entry_kind(&path) != Some(EntryKind::File) {
            return Ok(GlobalsFile::default());
        }
        GlobalsFile::parse(GLOBALS_FILE, &fsio::read_bounded(&path)?)
    }

    pub fn write_globals(&self, variables: VarTable) -> Result<(), AppError> {
        let file = GlobalsFile {
            variables,
            ..self.globals()?
        };
        fsio::write_atomic(&self.globals_path(), file.to_yaml()?.as_bytes())?;
        Ok(())
    }
}
