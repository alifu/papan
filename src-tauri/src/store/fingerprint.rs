//! Content fingerprint of a collection, used to tell outside changes from our own writes
//! (plan §5). Files are hashed one at a time in sorted path order; past the byte budget a
//! file contributes only its size and modification time, past the file cap nothing more.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use super::collection::{is_listable, Collection, COLLECTION_FILE, MAX_DEPTH};
use super::environments::{ENVIRONMENTS_DIR, ENV_SUFFIX};
use super::fsio::{self, EntryKind, MAX_FILE_BYTES};
use super::paths::{self, FOLDER_FILE};

pub const MAX_FINGERPRINT_FILES: usize = 10_000;
pub const MAX_FINGERPRINT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
struct Limits {
    files: usize,
    bytes: u64,
}

const LIMITS: Limits = Limits {
    files: MAX_FINGERPRINT_FILES,
    bytes: MAX_FINGERPRINT_BYTES,
};

/// A listed path and, for files, where to read it.
type Item = (String, Option<PathBuf>);

impl Collection {
    /// Hash of the collection file, the environment files and everything under `requests/`.
    pub fn fingerprint(&self) -> u64 {
        fingerprint_with(self, LIMITS)
    }
}

fn fingerprint_with(collection: &Collection, limits: Limits) -> u64 {
    let mut items: Vec<Item> = vec![(
        COLLECTION_FILE.to_string(),
        Some(collection.root.join(COLLECTION_FILE)),
    )];
    items.extend(collection.env_stems().into_iter().map(|env| {
        let name = format!("{env}{ENV_SUFFIX}");
        let path = collection.environments_dir().join(&name);
        (format!("{ENVIRONMENTS_DIR}/{name}"), Some(path))
    }));
    let root = collection.requests_root();
    if fsio::entry_kind(&root) == Some(EntryKind::Dir) {
        collect(&root, "", 0, limits.files, &mut items);
    }
    items.sort();
    let mut hasher = DefaultHasher::new();
    let mut budget = limits.bytes;
    for (rel, path) in &items {
        rel.hash(&mut hasher);
        let Some(path) = path else { continue };
        match fsio::file_stamp(path) {
            Some((len, _)) if len <= budget && len <= MAX_FILE_BYTES => {
                budget -= len;
                fsio::read_bounded(path)
                    .unwrap_or_default()
                    .hash(&mut hasher);
            }
            stamp => stamp.hash(&mut hasher),
        }
    }
    hasher.finish()
}

fn collect(dir: &Path, rel: &str, depth: usize, max_files: usize, items: &mut Vec<Item>) {
    for entry in fsio::list_dir(dir).unwrap_or_default() {
        if items.len() >= max_files {
            return;
        }
        let path = paths::join_rel(rel, &entry.name);
        let abs = dir.join(&entry.name);
        let is_folder_file = entry.name == FOLDER_FILE && entry.kind == EntryKind::File;
        if is_folder_file || (is_listable(&entry) && entry.kind == EntryKind::File) {
            items.push((path, Some(abs)));
        } else if is_listable(&entry) && depth + 1 < MAX_DEPTH {
            items.push((format!("{path}/"), None));
            collect(&abs, &path, depth + 1, max_files, items);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::types::RequestKind;

    fn collection() -> (tempfile::TempDir, Collection) {
        let dir = tempfile::tempdir().unwrap();
        let collection = Collection::create(dir.path(), "F").unwrap();
        (dir, collection)
    }

    #[test]
    fn past_the_byte_budget_size_changes_are_still_seen() {
        let (_dir, collection) = collection();
        let file = collection.requests_root().join("a.http.yaml");
        fsio::write_atomic(&file, b"kind: http\nmethod: GET\nname: A\n").unwrap();
        let no_bytes = Limits {
            files: 100,
            bytes: 0,
        };
        let before = fingerprint_with(&collection, no_bytes);

        fsio::write_atomic(&file, b"kind: http\nmethod: POST\nname: A\n").unwrap();

        assert_ne!(fingerprint_with(&collection, no_bytes), before);
    }

    #[test]
    fn file_cap_stops_collecting_but_stays_deterministic() {
        let (_dir, collection) = collection();
        for name in ["A", "B", "C"] {
            collection
                .create_request("", name, RequestKind::Http)
                .unwrap();
        }
        let one = Limits {
            files: 1,
            bytes: u64::MAX,
        };

        assert_eq!(
            fingerprint_with(&collection, one),
            fingerprint_with(&collection, one)
        );
        assert_ne!(fingerprint_with(&collection, one), collection.fingerprint());
    }
}
