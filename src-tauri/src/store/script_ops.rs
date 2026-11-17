//! Folder and collection scripts (plan 05 §3, §5): read from and written to
//! `_folder.yaml` and `papan.collection.yaml`. Request scripts travel in
//! `RequestContent`. Every write refreshes the fingerprint, so our own writes fire no
//! `CollectionChanged`.

use super::collection::{read_folder_file, write_folder_file, Collection, COLLECTION_FILE};
use super::fsio;
use super::paths::FOLDER_FILE;
use super::script_model::validate_scripts;
use super::types::{ScriptTarget, Scripts};
use super::Store;
use crate::error::AppError;

fn root_folder_error() -> AppError {
    AppError::invalid_input("Use the collection for scripts that apply to the whole collection.")
}

impl Collection {
    pub fn read_folder_scripts(&self, rel: &str) -> Result<Scripts, AppError> {
        if rel.is_empty() {
            return Err(root_folder_error());
        }
        read_folder_file(&self.dir_at(rel)?)?.scripts(FOLDER_FILE)
    }

    pub fn write_folder_scripts(&self, rel: &str, scripts: &Scripts) -> Result<(), AppError> {
        if rel.is_empty() {
            return Err(root_folder_error());
        }
        validate_scripts(FOLDER_FILE, scripts)?;
        let dir = self.dir_at(rel)?;
        write_folder_file(&dir, &read_folder_file(&dir)?.with_scripts(scripts))
    }

    pub fn read_collection_scripts(&self) -> Result<Scripts, AppError> {
        self.read_collection_file()?.scripts(COLLECTION_FILE)
    }

    pub fn write_collection_scripts(&self, scripts: &Scripts) -> Result<(), AppError> {
        validate_scripts(COLLECTION_FILE, scripts)?;
        let file = self.read_collection_file()?.with_scripts(scripts);
        fsio::write_atomic(&self.root.join(COLLECTION_FILE), file.to_yaml()?.as_bytes())?;
        Ok(())
    }
}

impl Store {
    pub fn read_scripts(&self, target: &ScriptTarget) -> Result<Scripts, AppError> {
        let collection = self.collection(collection_id(target))?;
        match target {
            ScriptTarget::Collection { .. } => collection.read_collection_scripts(),
            ScriptTarget::Folder { path, .. } => collection.read_folder_scripts(path),
        }
    }

    pub fn write_scripts(
        &mut self,
        target: &ScriptTarget,
        scripts: &Scripts,
    ) -> Result<(), AppError> {
        let id = collection_id(target);
        let collection = self.collection(id)?;
        let result = match target {
            ScriptTarget::Collection { .. } => collection.write_collection_scripts(scripts),
            ScriptTarget::Folder { path, .. } => collection.write_folder_scripts(path, scripts),
        };
        self.remember(id, &collection);
        result
    }
}

fn collection_id(target: &ScriptTarget) -> &str {
    match target {
        ScriptTarget::Collection { collection_id } | ScriptTarget::Folder { collection_id, .. } => {
            collection_id
        }
    }
}
