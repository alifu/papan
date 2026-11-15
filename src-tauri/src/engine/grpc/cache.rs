//! The reflection descriptor cache (plan 08 §5, §11 D4):
//! `<app data>/grpc-descriptors/<key>.bin`, never in a collection. A file is one JSON
//! header line (`endpoint`, `fetched_at_ms`) followed by the encoded `FileDescriptorSet`.
//! `key` is the hex SHA-256 of `grant_scope + "\n" + scheme://host:port` (no query, no
//! credentials). At most 50 files and 50 MiB; the oldest go first. A damaged file is
//! deleted and reads as "no cache".

use std::path::{Path, PathBuf};

use aws_lc_rs::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::store::fsio;

pub const MAX_ENTRIES: usize = 50;
pub const MAX_TOTAL_BYTES: u64 = 50 * 1024 * 1024;
const DIR: &str = "grpc-descriptors";
const SUFFIX: &str = ".bin";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Header {
    endpoint: String,
    fetched_at_ms: f64,
}

/// A cached `FileDescriptorSet`, still encoded.
#[derive(Debug, Clone, PartialEq)]
pub struct CachedSet {
    pub fetched_at_ms: f64,
    pub encoded: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct DescriptorCache {
    dir: PathBuf,
}

/// The cache key of `endpoint` (`scheme://host:port`) for `scope`.
pub fn key(scope: &str, endpoint: &str) -> String {
    let hash = digest(&SHA256, format!("{scope}\n{endpoint}").as_bytes());
    hash.as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn parse(bytes: &[u8], endpoint: &str) -> Option<CachedSet> {
    let split = bytes.iter().position(|byte| *byte == b'\n')?;
    let header: Header = serde_json::from_slice(&bytes[..split]).ok()?;
    (header.endpoint == endpoint).then(|| CachedSet {
        fetched_at_ms: header.fetched_at_ms,
        encoded: bytes[split + 1..].to_vec(),
    })
}

impl DescriptorCache {
    pub fn new(app_data: &Path) -> Self {
        Self {
            dir: app_data.join(DIR),
        }
    }

    fn path(&self, scope: &str, endpoint: &str) -> PathBuf {
        self.dir.join(format!("{}{SUFFIX}", key(scope, endpoint)))
    }

    /// The cached set, or `None` (also for a damaged file, which is deleted).
    pub fn read(&self, scope: &str, endpoint: &str) -> Option<CachedSet> {
        let path = self.path(scope, endpoint);
        let bytes = std::fs::read(&path).ok()?;
        let parsed = parse(&bytes, endpoint);
        if parsed.is_none() {
            self.remove(scope, endpoint);
        }
        parsed
    }

    /// Deletes the entry: after a damaged read, or when its descriptors turn out unusable.
    pub fn remove(&self, scope: &str, endpoint: &str) {
        tracing::warn!("deleting a damaged gRPC descriptor cache file");
        let _ = std::fs::remove_file(self.path(scope, endpoint));
    }

    pub fn write(&self, scope: &str, endpoint: &str, set: &CachedSet) -> Result<(), AppError> {
        fsio::create_dir_all(&self.dir)?;
        let header = Header {
            endpoint: endpoint.to_string(),
            fetched_at_ms: set.fetched_at_ms,
        };
        let mut bytes = serde_json::to_vec(&header).map_err(|source| {
            AppError::internal("Could not store the descriptors.").with_detail(source.to_string())
        })?;
        bytes.push(b'\n');
        bytes.extend_from_slice(&set.encoded);
        fsio::write_atomic(&self.path(scope, endpoint), &bytes)?;
        self.prune();
        Ok(())
    }

    /// Drops the oldest files beyond [`MAX_ENTRIES`] or [`MAX_TOTAL_BYTES`].
    fn prune(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.to_string_lossy().ends_with(SUFFIX))
            .filter_map(|path| {
                let meta = std::fs::metadata(&path).ok()?;
                Some((meta.modified().ok()?, meta.len(), path))
            })
            .collect();
        // Newest first: keep while within both limits, delete the rest.
        files.sort_by_key(|file| std::cmp::Reverse(file.0));
        let mut total = 0_u64;
        for (index, (_, size, path)) in files.into_iter().enumerate() {
            total += size;
            if index >= MAX_ENTRIES || total > MAX_TOTAL_BYTES {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;

    const SCOPE: &str = "collection:c1";
    const ENDPOINT: &str = "https://api.example.com:443";

    fn set(byte: u8, len: usize) -> CachedSet {
        CachedSet {
            fetched_at_ms: 1_700_000_000_000.0,
            encoded: vec![byte; len],
        }
    }

    fn count(dir: &Path) -> usize {
        std::fs::read_dir(dir.join(DIR)).unwrap().count()
    }

    #[test]
    fn a_written_set_is_read_back_per_scope_and_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DescriptorCache::new(dir.path());
        // Bytes that look like a line break must survive.
        let stored = CachedSet {
            encoded: b"\n\x00\n\xff".to_vec(),
            ..set(0, 0)
        };

        cache.write(SCOPE, ENDPOINT, &stored).unwrap();

        assert_eq!(cache.read(SCOPE, ENDPOINT), Some(stored));
        assert_eq!(cache.read("scratch:s1", ENDPOINT), None);
        assert_eq!(cache.read(SCOPE, "https://other:443"), None);
    }

    #[test]
    fn the_key_depends_on_scope_and_endpoint_only() {
        let a = key(SCOPE, ENDPOINT);

        assert_eq!(a.len(), 64);
        assert_eq!(a, key(SCOPE, ENDPOINT));
        assert_ne!(a, key("collection:c2", ENDPOINT));
        assert_ne!(a, key(SCOPE, "http://api.example.com:443"));
    }

    #[test]
    fn a_damaged_file_is_deleted_and_reads_as_no_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DescriptorCache::new(dir.path());
        cache.write(SCOPE, ENDPOINT, &set(1, 8)).unwrap();
        let path = cache.path(SCOPE, ENDPOINT);
        for damaged in [
            &b"no header line"[..],
            b"{not json}\nrest",
            b"{\"endpoint\":\"x\",\"fetched_at_ms\":1}\n",
        ] {
            std::fs::write(&path, damaged).unwrap();

            assert_eq!(cache.read(SCOPE, ENDPOINT), None);
            assert!(!path.exists());
        }
    }

    #[test]
    fn the_oldest_files_are_dropped_beyond_the_count() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DescriptorCache::new(dir.path());
        let old = SystemTime::now() - Duration::from_secs(3600);
        for n in 0..MAX_ENTRIES {
            let endpoint = format!("http://h{n}:1");
            cache.write(SCOPE, &endpoint, &set(1, 4)).unwrap();
            let file = std::fs::File::options()
                .write(true)
                .open(cache.path(SCOPE, &endpoint))
                .unwrap();
            file.set_modified(old + Duration::from_secs(n as u64))
                .unwrap();
        }

        cache.write(SCOPE, ENDPOINT, &set(1, 4)).unwrap();

        assert_eq!(count(dir.path()), MAX_ENTRIES);
        assert!(cache.read(SCOPE, "http://h0:1").is_none());
        assert!(cache.read(SCOPE, "http://h1:1").is_some());
        assert!(cache.read(SCOPE, ENDPOINT).is_some());
    }

    #[test]
    fn the_total_size_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DescriptorCache::new(dir.path());
        let big = usize::try_from(MAX_TOTAL_BYTES / 2).unwrap();
        let old = SystemTime::now() - Duration::from_secs(3600);
        for n in 0..2_u64 {
            let endpoint = format!("http://big{n}:1");
            cache.write(SCOPE, &endpoint, &set(1, big)).unwrap();
            let file = std::fs::File::options()
                .write(true)
                .open(cache.path(SCOPE, &endpoint))
                .unwrap();
            file.set_modified(old + Duration::from_secs(n)).unwrap();
        }

        assert_eq!(count(dir.path()), 1);
        assert!(cache.read(SCOPE, "http://big1:1").is_some());
    }
}
