//! `<collection>/journeys/<slug>.journey.yaml`, one journey per file, keys sorted so Git
//! diffs stay small (plan 13 §3). Every file name from the webview is checked.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::model::{validate, Journey};
use super::types::JourneySummary;
use crate::error::AppError;
use crate::store::fsio::{self, EntryKind};
use crate::store::paths::{is_valid_segment, slugify, unique_stem};
use crate::store::yaml::{self, Node};

pub const JOURNEYS_DIR: &str = "journeys";
pub const SUFFIX: &str = ".journey.yaml";

fn dir_of(root: &Path) -> PathBuf {
    root.join(JOURNEYS_DIR)
}

/// A `journeys` link could lead outside the collection; reads and writes refuse it.
fn check_dir(root: &Path) -> Result<(), AppError> {
    if fsio::entry_kind(&dir_of(root)) == Some(EntryKind::Symlink) {
        return Err(AppError::invalid_input(
            "The journeys folder is a link. Replace it with a real folder.",
        ));
    }
    Ok(())
}

/// A journey file name: safe segment ending in `.journey.yaml`.
pub fn check_file(file: &str) -> Result<&str, AppError> {
    let valid = file.len() > SUFFIX.len() && file.ends_with(SUFFIX) && is_valid_segment(file);
    if valid {
        Ok(file)
    } else {
        Err(AppError::invalid_input(
            "That journey file name is not allowed.",
        ))
    }
}

fn path_of(root: &Path, file: &str) -> Result<PathBuf, AppError> {
    Ok(dir_of(root).join(check_file(file)?))
}

pub fn read(root: &Path, file: &str) -> Result<Journey, AppError> {
    check_dir(root)?;
    let path = path_of(root, file)?;
    let bytes = fsio::read_bounded(&path)?;
    let journey: Journey = yaml::parse(file, &bytes)?;
    validate(file, &journey)?;
    Ok(journey)
}

/// Every `*.journey.yaml` file with its summary; an unreadable file is listed with its error.
pub fn list(root: &Path) -> Vec<JourneySummary> {
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
            Ok(journey) => JourneySummary::of(&file, &journey),
            Err(error) => JourneySummary::broken(&file, &error.message),
        })
        .collect()
}

fn emit(journey: &Journey) -> Result<String, AppError> {
    let value = serde_json::to_value(journey).map_err(|source| {
        AppError::internal("Could not write the journey.").with_detail(source.to_string())
    })?;
    let serde_json::Value::Object(map) = &value else {
        return Err(AppError::internal("Could not write the journey."));
    };
    let nodes: BTreeMap<String, Node> = map
        .iter()
        .map(|(k, v)| (k.clone(), Node::from(v)))
        .collect();
    yaml::emit(&nodes)
}

/// Writes the journey; a new one (`file` is `None`) gets a file named after it.
/// Returns the file name.
pub fn write(root: &Path, file: Option<&str>, journey: &Journey) -> Result<String, AppError> {
    let name = match file {
        Some(file) => check_file(file)?.to_string(),
        None => new_file_name(root, &journey.name)?,
    };
    validate(&name, journey)?;
    check_dir(root)?;
    let text = emit(journey)?;
    fsio::create_dir_all(&dir_of(root))?;
    fsio::write_atomic(&path_of(root, &name)?, text.as_bytes())?;
    Ok(name)
}

fn new_file_name(root: &Path, journey_name: &str) -> Result<String, AppError> {
    let existing: Vec<String> = fsio::list_dir(&dir_of(root))
        .map(|entries| entries.into_iter().map(|e| e.name).collect())
        .unwrap_or_default();
    let stem = unique_stem(&slugify(journey_name), &existing)?;
    Ok(format!("{stem}{SUFFIX}"))
}

pub fn delete(root: &Path, file: &str) -> Result<(), AppError> {
    check_dir(root)?;
    fsio::remove_file(&path_of(root, file)?)
}

#[cfg(test)]
mod tests {
    use super::super::model::tests_support::sample;
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_linked_journeys_folder_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("journeys")).unwrap();
        assert!(write(dir.path(), None, &sample()).is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        assert!(read(dir.path(), "signup.journey.yaml").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_journeys_folder_is_not_deleted_from() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("x.journey.yaml"), "name: x").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("journeys")).unwrap();
        assert!(delete(dir.path(), "x.journey.yaml").is_err());
        assert!(outside.path().join("x.journey.yaml").exists());
    }

    #[test]
    fn writes_reads_and_lists_with_stable_output() {
        let dir = tempfile::tempdir().unwrap();
        let name = write(dir.path(), None, &sample()).unwrap();
        assert_eq!(name, "signup.journey.yaml");
        assert_eq!(read(dir.path(), &name).unwrap(), sample());
        let first = std::fs::read_to_string(dir.path().join("journeys").join(&name)).unwrap();
        write(dir.path(), Some(&name), &sample()).unwrap();
        let second = std::fs::read_to_string(dir.path().join("journeys").join(&name)).unwrap();
        assert_eq!(first, second);
        let other = write(dir.path(), None, &sample()).unwrap();
        assert_eq!(other, "signup-2.journey.yaml");
        let listed = list(dir.path());
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].nodes, 3);
        delete(dir.path(), &name).unwrap();
        assert_eq!(list(dir.path()).len(), 1);
    }

    #[test]
    fn bad_names_and_broken_files_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path(), "../x.journey.yaml").is_err());
        assert!(read(dir.path(), "x.yaml").is_err());
        let mut bad = sample();
        bad.nodes[1].request = "../escape".into();
        assert!(write(dir.path(), None, &bad).is_err());
        std::fs::create_dir_all(dir.path().join("journeys")).unwrap();
        std::fs::write(dir.path().join("journeys/broken.journey.yaml"), "name: [").unwrap();
        let listed = list(dir.path());
        assert!(listed[0].error.is_some());
    }
}
