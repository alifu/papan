//! The WSDL model cache (plan 06 §7.1): `<app data>/wsdl/<key>.json`, never in a
//! collection. It stores the parsed model, not the raw documents. `key` is the hex
//! SHA-256 of `grant_scope + "\n" + source`. At most 100 entries and 50 MiB; the oldest
//! go first. A damaged entry is deleted and reads as "no cache".

use std::path::{Path, PathBuf};

use aws_lc_rs::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};

use super::model::WsdlModel;
use crate::error::AppError;
use crate::store::fsio;

pub const MAX_ENTRIES: usize = 100;
pub const MAX_TOTAL_BYTES: u64 = 50 * 1024 * 1024;
const DIR: &str = "wsdl";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheEntry {
    pub scope: String,
    /// The source as shown: a URL without credentials, or a file name.
    pub source: String,
    /// The picked file's name for a file source, else empty.
    pub file_name: String,
    pub fetched: Vec<String>,
    pub warnings: Vec<String>,
    pub fetched_at_ms: f64,
    pub model: WsdlModel,
}

#[derive(Debug, Clone)]
pub struct WsdlCache {
    dir: PathBuf,
}

/// The cache key of `source` imported for `scope`.
pub fn key(scope: &str, source: &str) -> String {
    let hash = digest(&SHA256, format!("{scope}\n{source}").as_bytes());
    hash.as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A key from the webview is a file name: it must be exactly 64 lowercase hex digits.
pub fn validate_key(key: &str) -> Result<&str, AppError> {
    let valid = key.len() == 64 && key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if valid {
        Ok(key)
    } else {
        Err(AppError::invalid_input("That WSDL id is not valid."))
    }
}

impl WsdlCache {
    pub fn new(app_data: &Path) -> Self {
        Self {
            dir: app_data.join(DIR),
        }
    }

    fn path(&self, key: &str) -> Result<PathBuf, AppError> {
        Ok(self.dir.join(format!("{}.json", validate_key(key)?)))
    }

    pub fn store(&self, key: &str, entry: &CacheEntry) -> Result<(), AppError> {
        let path = self.path(key)?;
        std::fs::create_dir_all(&self.dir)?;
        let bytes = serde_json::to_vec(entry).map_err(|source| {
            AppError::internal("Could not save the WSDL cache.").with_detail(source.to_string())
        })?;
        fsio::write_atomic(&path, &bytes)?;
        self.prune(&path);
        Ok(())
    }

    /// The entry under `key`; a damaged file is deleted.
    pub fn load(&self, key: &str) -> Result<Option<CacheEntry>, AppError> {
        let path = self.path(key)?;
        let Ok(bytes) = std::fs::read(&path) else {
            return Ok(None);
        };
        match serde_json::from_slice(&bytes) {
            Ok(entry) => Ok(Some(entry)),
            Err(error) => {
                tracing::warn!("Deleting a damaged WSDL cache entry: {error}");
                let _ = std::fs::remove_file(&path);
                Ok(None)
            }
        }
    }

    /// The newest entry for a picked file named `file_name` in `scope`.
    pub fn find_file(&self, scope: &str, file_name: &str) -> Option<(String, CacheEntry)> {
        self.files()
            .into_iter()
            .rev()
            .filter_map(|(path, _, _)| {
                let key = path.file_stem()?.to_string_lossy().into_owned();
                let entry = self.load(&key).ok()??;
                (entry.scope == scope && !file_name.is_empty() && entry.file_name == file_name)
                    .then_some((key, entry))
            })
            .next()
    }

    /// Cache files, oldest first: `(path, modified, size)`.
    fn files(&self) -> Vec<(PathBuf, std::time::SystemTime, u64)> {
        let Ok(read) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut files: Vec<_> = read
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .filter_map(|entry| {
                let metadata = entry.metadata().ok()?;
                Some((entry.path(), metadata.modified().ok()?, metadata.len()))
            })
            .collect();
        files.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        files
    }

    /// Drops the oldest entries over the caps; `kept` (just written) always stays.
    fn prune(&self, kept: &Path) {
        let files = self.files();
        let mut count = files.len();
        let mut total: u64 = files.iter().map(|(_, _, size)| size).sum();
        for (path, _, size) in files {
            if count <= MAX_ENTRIES && total <= MAX_TOTAL_BYTES {
                break;
            }
            if path == kept {
                continue;
            }
            if std::fs::remove_file(&path).is_ok() {
                count -= 1;
                total = total.saturating_sub(size);
            }
        }
    }
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;
