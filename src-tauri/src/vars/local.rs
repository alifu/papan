//! `<app data>/vars/<collection id>.json`: the selected environment and the local
//! "current values" of a collection (plan 04 §3.3). Never in Git.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::store::fsio::{self, EntryKind};

const VARS_DIR: &str = "vars";
const LOCAL_VERSION: u32 = 1;

pub type Values = BTreeMap<String, String>;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Current {
    #[serde(default)]
    pub collection: Values,
    #[serde(default)]
    pub environments: BTreeMap<String, Values>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalState {
    pub version: u32,
    #[serde(default)]
    pub selected_env: Option<String>,
    #[serde(default)]
    pub current: Current,
}

impl Default for LocalState {
    fn default() -> Self {
        Self {
            version: LOCAL_VERSION,
            selected_env: None,
            current: Current::default(),
        }
    }
}

/// The state file of `collection_id`, which must be a UUID before it names a file.
fn state_path(app_data: &Path, collection_id: &str) -> Result<PathBuf, AppError> {
    let id = uuid::Uuid::parse_str(collection_id)
        .map_err(|_| AppError::invalid_input("That collection id is not valid."))?;
    Ok(app_data.join(VARS_DIR).join(format!("{id}.json")))
}

/// Loads the state; a missing or malformed file reads as empty (logged).
pub fn load(app_data: &Path, collection_id: &str) -> Result<LocalState, AppError> {
    let path = state_path(app_data, collection_id)?;
    if fsio::entry_kind(&path).is_none() {
        return Ok(LocalState::default());
    }
    let parsed = fsio::read_bounded(&path).and_then(|bytes| {
        serde_json::from_slice::<LocalState>(&bytes).map_err(|source| {
            AppError::invalid_input("A local variables file is malformed.")
                .with_detail(source.to_string())
        })
    });
    Ok(parsed.unwrap_or_else(|error| {
        tracing::warn!(detail = ?error.detail, "{error} Using no local values.");
        LocalState::default()
    }))
}

pub fn save(app_data: &Path, collection_id: &str, state: &LocalState) -> Result<(), AppError> {
    let path = state_path(app_data, collection_id)?;
    let dir = app_data.join(VARS_DIR);
    if fsio::entry_kind(&dir) != Some(EntryKind::Dir) {
        fsio::create_dir_all(&dir)?;
    }
    let text = serde_json::to_string(state).map_err(|source| {
        AppError::internal("Could not save local variables.").with_detail(source.to_string())
    })?;
    fsio::write_atomic(&path, format!("{text}\n").as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_tolerates_a_malformed_file() {
        let dir = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let state = LocalState {
            selected_env: Some("dev".into()),
            ..LocalState::default()
        };

        assert_eq!(load(dir.path(), &id).unwrap(), LocalState::default());
        save(dir.path(), &id, &state).unwrap();
        assert_eq!(load(dir.path(), &id).unwrap(), state);

        fsio::write_atomic(&state_path(dir.path(), &id).unwrap(), b"{nope").unwrap();
        assert_eq!(load(dir.path(), &id).unwrap(), LocalState::default());
    }

    #[test]
    fn a_non_uuid_id_never_names_a_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path(), "../x").is_err());
        assert!(save(dir.path(), "../x", &LocalState::default()).is_err());
    }
}
