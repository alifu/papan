//! The schema cache (plan 06 §6): `<app data>/graphql-schemas/<key>.json`, never in a
//! collection. `key` is the hex SHA-256 of `grant_scope + "\n" + endpoint`, where the
//! endpoint is scheme, host, port and path (no query, so no keys). At most 50 files; the
//! oldest go first. A damaged file is deleted and reads as "no cache".

use std::path::{Path, PathBuf};

use aws_lc_rs::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};

use super::types::{GraphqlSchema, SchemaSource};
use crate::error::AppError;
use crate::store::fsio;

pub const MAX_ENTRIES: usize = 50;
const DIR: &str = "graphql-schemas";
const SUFFIX: &str = ".json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Source {
    Introspection,
    Sdl,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    endpoint: String,
    fetched_at_ms: f64,
    source: Source,
    text: String,
}

#[derive(Debug, Clone)]
pub struct SchemaCache {
    dir: PathBuf,
}

/// The cache key of `endpoint` for `scope`.
pub fn key(scope: &str, endpoint: &str) -> String {
    let hash = digest(&SHA256, format!("{scope}\n{endpoint}").as_bytes());
    hash.as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl From<Entry> for GraphqlSchema {
    fn from(entry: Entry) -> Self {
        let source = match entry.source {
            Source::Introspection => SchemaSource::Introspection { json: entry.text },
            Source::Sdl => SchemaSource::Sdl { text: entry.text },
        };
        Self {
            endpoint: entry.endpoint,
            fetched_at_ms: entry.fetched_at_ms,
            source,
        }
    }
}

fn entry_of(schema: &GraphqlSchema) -> Entry {
    let (source, text) = match &schema.source {
        SchemaSource::Introspection { json } => (Source::Introspection, json.clone()),
        SchemaSource::Sdl { text } => (Source::Sdl, text.clone()),
    };
    Entry {
        endpoint: schema.endpoint.clone(),
        fetched_at_ms: schema.fetched_at_ms,
        source,
        text,
    }
}

impl SchemaCache {
    pub fn new(app_data: &Path) -> Self {
        Self {
            dir: app_data.join(DIR),
        }
    }

    fn path(&self, scope: &str, endpoint: &str) -> PathBuf {
        self.dir.join(format!("{}{SUFFIX}", key(scope, endpoint)))
    }

    /// The cached schema, or `None` (also for a damaged file, which is deleted).
    pub fn read(&self, scope: &str, endpoint: &str) -> Option<GraphqlSchema> {
        let path = self.path(scope, endpoint);
        let bytes = std::fs::read(&path).ok()?;
        match serde_json::from_slice::<Entry>(&bytes) {
            Ok(entry) if entry.endpoint == endpoint => Some(entry.into()),
            _ => {
                tracing::warn!("deleting a damaged schema cache file");
                let _ = std::fs::remove_file(&path);
                None
            }
        }
    }

    pub fn write(&self, scope: &str, schema: &GraphqlSchema) -> Result<(), AppError> {
        fsio::create_dir_all(&self.dir)?;
        let bytes = serde_json::to_vec(&entry_of(schema)).map_err(|source| {
            AppError::internal("Could not store the schema.").with_detail(source.to_string())
        })?;
        fsio::write_atomic(&self.path(scope, &schema.endpoint), &bytes)?;
        self.prune();
        Ok(())
    }

    pub fn clear(&self, scope: &str, endpoint: &str) -> Result<(), AppError> {
        match std::fs::remove_file(self.path(scope, endpoint)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(()),
        }
    }

    /// Drops the oldest files beyond [`MAX_ENTRIES`].
    fn prune(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.to_string_lossy().ends_with(SUFFIX))
            .filter_map(|path| Some((std::fs::metadata(&path).ok()?.modified().ok()?, path)))
            .collect();
        files.sort();
        let excess = files.len().saturating_sub(MAX_ENTRIES);
        for (_, path) in files.into_iter().take(excess) {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema(endpoint: &str, at: f64) -> GraphqlSchema {
        GraphqlSchema {
            endpoint: endpoint.into(),
            fetched_at_ms: at,
            source: SchemaSource::Sdl {
                text: "type Query { a: Int }".into(),
            },
        }
    }

    #[test]
    fn keys_differ_by_scope_and_endpoint_and_are_hex() {
        let a = key("scratch", "https://x/graphql");

        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, key("collection:1", "https://x/graphql"));
        assert_ne!(a, key("scratch", "https://y/graphql"));
    }

    #[test]
    fn write_read_and_clear_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let cache = SchemaCache::new(dir.path());
        let stored = schema("https://x/graphql", 5.0);

        cache.write("s", &stored).unwrap();

        assert_eq!(cache.read("s", "https://x/graphql"), Some(stored));
        assert_eq!(cache.read("other", "https://x/graphql"), None);
        cache.clear("s", "https://x/graphql").unwrap();
        cache.clear("s", "https://x/graphql").unwrap();
        assert_eq!(cache.read("s", "https://x/graphql"), None);
    }

    #[test]
    fn a_damaged_file_reads_as_none_and_is_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let cache = SchemaCache::new(dir.path());
        let path = cache.path("s", "e");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{oops").unwrap();

        assert_eq!(cache.read("s", "e"), None);
        assert!(!path.exists());
    }

    #[test]
    fn only_the_newest_fifty_files_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let cache = SchemaCache::new(dir.path());
        let first = schema("https://x/0", 0.0);
        cache.write("s", &first).unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(cache.path("s", "https://x/0"))
            .unwrap()
            .set_modified(old)
            .unwrap();

        for n in 1..=MAX_ENTRIES {
            cache
                .write("s", &schema(&format!("https://x/{n}"), 1.0))
                .unwrap();
        }

        assert_eq!(std::fs::read_dir(&cache.dir).unwrap().count(), MAX_ENTRIES);
        assert_eq!(cache.read("s", "https://x/0"), None);
        assert!(cache.read("s", "https://x/1").is_some());
    }
}
