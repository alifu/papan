//! `<app data>/untrusted-collections.json`: collections whose scripts must not run until
//! the user says so. A collection that arrives from outside (a Git clone) starts here;
//! collections the user made or opened themselves are never listed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::fsio;
use crate::error::AppError;

const TRUST_FILE: &str = "untrusted-collections.json";

#[derive(Debug, Default, Serialize, Deserialize)]
struct TrustFile {
    untrusted: BTreeSet<String>,
}

#[derive(Debug)]
pub struct ScriptTrust {
    path: PathBuf,
    untrusted: BTreeSet<String>,
}

impl ScriptTrust {
    /// An unreadable file is logged and treated as empty; losing it cannot make a
    /// collection run *more* than it did before it was listed.
    pub fn load(app_data: &Path) -> Self {
        let path = app_data.join(TRUST_FILE);
        let untrusted = fsio::read_bounded(&path)
            .ok()
            .and_then(|bytes| {
                serde_json::from_slice::<TrustFile>(&bytes)
                    .map_err(|error| tracing::warn!(%error, "ignoring a malformed trust file"))
                    .ok()
            })
            .map(|file| file.untrusted)
            .unwrap_or_default();
        Self { path, untrusted }
    }

    pub fn is_trusted(&self, collection_id: &str) -> bool {
        !self.untrusted.contains(collection_id)
    }

    pub fn distrust(&mut self, collection_id: &str) -> Result<(), AppError> {
        self.update(|set| set.insert(collection_id.to_string()))
    }

    pub fn trust(&mut self, collection_id: &str) -> Result<(), AppError> {
        self.update(|set| set.remove(collection_id))
    }

    fn update(
        &mut self,
        change: impl FnOnce(&mut BTreeSet<String>) -> bool,
    ) -> Result<(), AppError> {
        let mut next = self.untrusted.clone();
        if !change(&mut next) {
            return Ok(());
        }
        let bytes = serde_json::to_vec_pretty(&TrustFile {
            untrusted: next.clone(),
        })
        .map_err(|source| {
            AppError::internal("Could not save trust settings.").with_detail(source.to_string())
        })?;
        fsio::write_atomic(&self.path, &bytes)?;
        self.untrusted = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distrust_persists_and_trust_clears() {
        let dir = tempfile::tempdir().unwrap();
        let mut trust = ScriptTrust::load(dir.path());
        assert!(trust.is_trusted("a"));
        trust.distrust("a").unwrap();
        assert!(!ScriptTrust::load(dir.path()).is_trusted("a"));
        trust.trust("a").unwrap();
        assert!(ScriptTrust::load(dir.path()).is_trusted("a"));
    }

    #[test]
    fn a_malformed_file_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(TRUST_FILE), "{nope").unwrap();
        assert!(ScriptTrust::load(dir.path()).is_trusted("a"));
    }
}
