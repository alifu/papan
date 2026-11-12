//! Scratch requests: `<app data>/scratch/<uuid>.http.yaml` (plan §6). Ids are generated
//! here and checked as UUIDs on every call, so they can never carry a path.

use std::path::{Path, PathBuf};

use super::fsio::{self, EntryKind};
use super::model::{self, RequestFile};
use super::paths::REQUEST_SUFFIX;
use super::types::{RequestContent, ScratchItem};
use crate::error::AppError;

pub const SCRATCH_DIR: &str = "scratch";
const DEFAULT_NAME: &str = "Untitled";
const DEFAULT_METHOD: &str = "GET";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scratch {
    dir: PathBuf,
}

/// The id when `text` is a lowercase hyphenated UUID, as we generate them.
fn parse_id(text: &str) -> Option<String> {
    uuid::Uuid::parse_str(text)
        .ok()
        .map(|id| id.hyphenated().to_string())
        .filter(|canonical| canonical == text)
}

impl Scratch {
    /// Uses (and creates if needed) `<app_data>/scratch`.
    pub fn new(app_data: &Path) -> Result<Self, AppError> {
        let dir = app_data.join(SCRATCH_DIR);
        fsio::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    fn path_of(&self, id: &str) -> Result<PathBuf, AppError> {
        parse_id(id)
            .map(|id| self.dir.join(format!("{id}{REQUEST_SUFFIX}")))
            .ok_or_else(|| {
                AppError::invalid_input("That scratch request id is not valid.")
                    .with_detail(format!("{id:?}"))
            })
    }

    /// Valid scratch requests, sorted by name (case-insensitive), then id.
    pub fn list(&self) -> Result<Vec<ScratchItem>, AppError> {
        let mut items: Vec<ScratchItem> = fsio::list_dir(&self.dir)?
            .into_iter()
            .filter(|entry| entry.kind == EntryKind::File)
            .filter_map(|entry| parse_id(entry.name.strip_suffix(REQUEST_SUFFIX)?))
            .filter_map(|id| match self.read(&id) {
                Ok(file) => Some(item(id, &file)),
                Err(error) => {
                    tracing::warn!(%id, detail = ?error.detail, "skipping scratch request: {error}");
                    None
                }
            })
            .collect();
        items.sort_by(|a, b| (a.name.to_lowercase(), &a.id).cmp(&(b.name.to_lowercase(), &b.id)));
        Ok(items)
    }

    pub fn create(&self) -> Result<ScratchItem, AppError> {
        let id = uuid::Uuid::new_v4().to_string();
        let file = RequestFile::new(DEFAULT_NAME.to_string(), DEFAULT_METHOD);
        fsio::write_atomic(&self.path_of(&id)?, file.to_yaml()?.as_bytes())?;
        Ok(item(id, &file))
    }

    /// A new scratch request holding `content` (used to restore a history entry).
    pub fn create_from(
        &self,
        name: &str,
        content: &RequestContent,
    ) -> Result<ScratchItem, AppError> {
        let id = uuid::Uuid::new_v4().to_string();
        let name = model::clean_name(name).unwrap_or_else(|_| DEFAULT_NAME.to_string());
        let file = RequestFile::new(name, DEFAULT_METHOD).with_content(content)?;
        self.save(&id, &file)?;
        Ok(item(id, &file))
    }

    pub fn read(&self, id: &str) -> Result<RequestFile, AppError> {
        let path = self.path_of(id)?;
        let bytes = fsio::read_bounded(&path)
            .map_err(|_| AppError::not_found("That scratch request does not exist."))?;
        RequestFile::parse(&format!("{id}{REQUEST_SUFFIX}"), &bytes)
    }

    pub fn write(&self, id: &str, content: &RequestContent) -> Result<(), AppError> {
        let updated = self.read(id)?.with_content(content)?;
        self.save(id, &updated)
    }

    pub fn rename(&self, id: &str, name: &str) -> Result<Vec<ScratchItem>, AppError> {
        let name = model::clean_name(name)?;
        let updated = self.read(id)?.with_name(&name);
        self.save(id, &updated)?;
        self.list()
    }

    pub fn delete(&self, id: &str) -> Result<Vec<ScratchItem>, AppError> {
        let path = self.path_of(id)?;
        if fsio::entry_kind(&path) != Some(EntryKind::File) {
            return Err(AppError::not_found("That scratch request does not exist."));
        }
        fsio::remove_file(&path)?;
        self.list()
    }

    fn save(&self, id: &str, file: &RequestFile) -> Result<(), AppError> {
        fsio::write_atomic(&self.path_of(id)?, file.to_yaml()?.as_bytes()).map(|_| ())
    }
}

fn item(id: String, file: &RequestFile) -> ScratchItem {
    ScratchItem {
        id,
        name: file.name.clone(),
        method: file.method.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    fn scratch() -> (tempfile::TempDir, Scratch) {
        let dir = tempfile::tempdir().unwrap();
        let scratch = Scratch::new(dir.path()).unwrap();
        (dir, scratch)
    }

    #[test]
    fn create_rename_write_delete() {
        let (_dir, scratch) = scratch();

        let created = scratch.create().unwrap();
        let renamed = scratch.rename(&created.id, " Users ").unwrap();
        scratch
            .write(&created.id, &RequestContent::new("POST", "https://x"))
            .unwrap();
        let read = scratch.read(&created.id).unwrap();
        let after_delete = scratch.delete(&created.id).unwrap();

        assert_eq!(
            (created.name.as_str(), created.method.as_str()),
            ("Untitled", "GET")
        );
        assert_eq!(renamed[0].name, "Users");
        assert_eq!(
            (read.method.as_str(), read.url.as_str()),
            ("POST", "https://x")
        );
        assert!(after_delete.is_empty());
        assert_eq!(
            scratch.delete(&created.id).unwrap_err().code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn list_sorts_by_name_case_insensitively() {
        let (_dir, scratch) = scratch();
        for name in ["beta", "Alpha", "gamma"] {
            let item = scratch.create().unwrap();
            scratch.rename(&item.id, name).unwrap();
        }

        let names: Vec<String> = scratch
            .list()
            .unwrap()
            .into_iter()
            .map(|i| i.name)
            .collect();

        assert_eq!(names, ["Alpha", "beta", "gamma"]);
    }

    #[test]
    fn non_uuid_ids_are_rejected() {
        let (_dir, scratch) = scratch();
        let upper = uuid::Uuid::new_v4().to_string().to_uppercase();

        for id in ["../x", "", "abc", upper.as_str()] {
            assert_eq!(
                scratch.read(id).unwrap_err().code,
                ErrorCode::InvalidInput,
                "{id}"
            );
            assert_eq!(
                scratch.delete(id).unwrap_err().code,
                ErrorCode::InvalidInput
            );
        }
    }

    #[test]
    fn invalid_files_are_skipped_in_the_list() {
        let (dir, scratch) = scratch();
        let id = uuid::Uuid::new_v4().to_string();
        let bad = dir
            .path()
            .join(SCRATCH_DIR)
            .join(format!("{id}{REQUEST_SUFFIX}"));
        fsio::write_atomic(&bad, b"not: [valid").unwrap();
        fsio::write_atomic(&dir.path().join(SCRATCH_DIR).join("x.http.yaml"), b"").unwrap();

        assert!(scratch.list().unwrap().is_empty());
        assert_eq!(scratch.read(&id).unwrap_err().code, ErrorCode::InvalidInput);
    }
}
