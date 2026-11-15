//! `<collection>/mocks/<slug>.mock.yaml`, one mock environment per file, keys sorted so
//! Git diffs stay small (plan 11 §2). Every file name from the webview is checked.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::model::{validate, MockEnvironment};
use super::types::MockSummary;
use crate::error::AppError;
use crate::store::fsio::{self, EntryKind};
use crate::store::paths::{is_valid_segment, slugify, unique_stem};
use crate::store::yaml::{self, Node};

pub const MOCKS_DIR: &str = "mocks";
pub const SUFFIX: &str = ".mock.yaml";

fn dir_of(root: &Path) -> PathBuf {
    root.join(MOCKS_DIR)
}

/// A `mocks` link could lead outside the collection; reads and writes refuse it.
fn check_dir(root: &Path) -> Result<(), AppError> {
    if fsio::entry_kind(&dir_of(root)) == Some(EntryKind::Symlink) {
        return Err(AppError::invalid_input(
            "The mocks folder is a link. Replace it with a real folder.",
        ));
    }
    Ok(())
}

/// A mock file name: safe segment ending in `.mock.yaml`.
pub fn check_file(file: &str) -> Result<&str, AppError> {
    let valid = file.len() > SUFFIX.len() && file.ends_with(SUFFIX) && is_valid_segment(file);
    if valid {
        Ok(file)
    } else {
        Err(AppError::invalid_input(
            "That mock file name is not allowed.",
        ))
    }
}

fn path_of(root: &Path, file: &str) -> Result<PathBuf, AppError> {
    Ok(dir_of(root).join(check_file(file)?))
}

pub fn read(root: &Path, file: &str) -> Result<MockEnvironment, AppError> {
    check_dir(root)?;
    let path = path_of(root, file)?;
    let bytes = fsio::read_bounded(&path)?;
    let mock: MockEnvironment = yaml::parse(file, &bytes)?;
    validate(file, &mock)?;
    Ok(mock)
}

/// Every `*.mock.yaml` file with its summary; an unreadable file is listed with its error.
pub fn list(root: &Path) -> Vec<MockSummary> {
    let dir = dir_of(root);
    if fsio::entry_kind(&dir) != Some(EntryKind::Dir) {
        return Vec::new();
    }
    let mut names: Vec<String> = fsio::list_dir(&dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| entry.kind == EntryKind::File && check_file(&entry.name).is_ok())
        .map(|entry| entry.name)
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|file| match read(root, &file) {
            Ok(mock) => MockSummary::of(&file, &mock),
            Err(error) => MockSummary::broken(&file, &error.message),
        })
        .collect()
}

fn emit(mock: &MockEnvironment) -> Result<String, AppError> {
    let value = serde_json::to_value(mock).map_err(|source| {
        AppError::internal("Could not write the mock.").with_detail(source.to_string())
    })?;
    let serde_json::Value::Object(map) = &value else {
        return Err(AppError::internal("Could not write the mock."));
    };
    let nodes: BTreeMap<String, Node> = map
        .iter()
        .map(|(k, v)| (k.clone(), Node::from(v)))
        .collect();
    yaml::emit(&nodes)
}

/// Writes the mock; a new one (`file` is `None`) gets a file named after it.
/// Returns the file name.
pub fn write(root: &Path, file: Option<&str>, mock: &MockEnvironment) -> Result<String, AppError> {
    let name = match file {
        Some(file) => check_file(file)?.to_string(),
        None => new_file_name(root, &mock.name)?,
    };
    validate(&name, mock)?;
    check_dir(root)?;
    let text = emit(mock)?;
    fsio::create_dir_all(&dir_of(root))?;
    fsio::write_atomic(&path_of(root, &name)?, text.as_bytes())?;
    Ok(name)
}

fn new_file_name(root: &Path, mock_name: &str) -> Result<String, AppError> {
    let existing: Vec<String> = fsio::list_dir(&dir_of(root))
        .map(|entries| entries.into_iter().map(|e| e.name).collect())
        .unwrap_or_default();
    let stem = unique_stem(&slugify(mock_name), &existing)?;
    Ok(format!("{stem}{SUFFIX}"))
}

pub fn delete(root: &Path, file: &str) -> Result<(), AppError> {
    fsio::remove_file(&path_of(root, file)?)
}

#[cfg(test)]
mod tests {
    use super::super::model::tests_support::sample;
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_linked_mocks_folder_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("mocks")).unwrap();
        assert!(write(dir.path(), None, &sample()).is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        assert!(read(dir.path(), "users.mock.yaml").is_err());
    }

    #[test]
    fn writes_reads_and_lists_with_stable_output() {
        let dir = tempfile::tempdir().unwrap();
        let file = write(dir.path(), None, &sample()).unwrap();
        assert_eq!(file, "users.mock.yaml");
        assert_eq!(read(dir.path(), &file).unwrap(), sample());
        let first = std::fs::read_to_string(dir.path().join("mocks").join(&file)).unwrap();
        write(dir.path(), Some(&file), &sample()).unwrap();
        let second = std::fs::read_to_string(dir.path().join("mocks").join(&file)).unwrap();
        assert_eq!(first, second);
        let listed = list(dir.path());
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Users");
        assert_eq!(listed[0].routes, 1);
    }

    #[test]
    fn new_mocks_with_the_same_name_get_distinct_files() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(dir.path(), None, &sample()).unwrap();
        let b = write(dir.path(), None, &sample()).unwrap();
        assert_ne!(a, b);
        assert_eq!(b, "users-2.mock.yaml");
    }

    #[test]
    fn hostile_file_names_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        for bad in [
            "../x.mock.yaml",
            "a/b.mock.yaml",
            "x.yaml",
            ".mock.yaml",
            "..\\x.mock.yaml",
            "con.mock.yaml",
        ] {
            assert!(read(dir.path(), bad).is_err(), "{bad}");
            assert!(write(dir.path(), Some(bad), &sample()).is_err(), "{bad}");
            assert!(delete(dir.path(), bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn invalid_files_are_listed_with_their_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("mocks")).unwrap();
        std::fs::write(dir.path().join("mocks/bad.mock.yaml"), "name: [oops").unwrap();
        let listed = list(dir.path());
        assert_eq!(listed.len(), 1);
        assert!(listed[0].error.is_some());
    }

    #[test]
    fn invalid_mocks_are_not_saved_and_delete_removes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut mock = sample();
        mock.routes[0].responses[0].status = 1;
        assert!(write(dir.path(), None, &mock).is_err());
        assert!(!dir.path().join("mocks").exists() || list(dir.path()).is_empty());
        let file = write(dir.path(), None, &sample()).unwrap();
        delete(dir.path(), &file).unwrap();
        assert!(list(dir.path()).is_empty());
    }
}
