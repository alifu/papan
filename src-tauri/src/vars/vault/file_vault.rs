//! The encrypted vault file and its three modes (plan 04 §4.1, §4.3).
//!
//! - `keychain`: the key loaded (or was created on first use); changes are written.
//! - `session_only`: the key store cannot be used; secrets live in memory, nothing is written.
//! - `damaged`: a vault file exists but cannot be opened; it is left untouched until `reset`.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::crypto;
use super::keys::KeySource;
use super::{status_of, SecretChange, SecretStore};
use crate::error::AppError;
use crate::vars::types::{SecretStorageMode, SecretStorageStatus};

/// One secret value may not be larger than this.
pub const MAX_VALUE_BYTES: usize = 64 * 1024;
/// The vault file may not be larger than this.
pub const MAX_FILE_BYTES: usize = 5 * 1024 * 1024;

const PLAIN_VERSION: u32 = 1;
const TEMP_PREFIX: &str = ".papan-tmp-";
/// serde_json writes a control character as six bytes (`\u00XX`).
const JSON_WORST_CASE_GROWTH: usize = 6;

type Entries = BTreeMap<String, Zeroizing<String>>;

#[derive(Serialize)]
struct PlainOut<'a> {
    version: u32,
    entries: BTreeMap<&'a str, &'a str>,
}

#[derive(Deserialize)]
struct PlainIn {
    version: u32,
    entries: BTreeMap<String, String>,
}

/// The vault file cannot be read as a vault: wrong key, changed bytes, unknown format.
struct Unreadable;

enum Backing {
    Keychain(Zeroizing<Vec<u8>>),
    SessionOnly,
    Damaged,
}

struct Open {
    backing: Backing,
    entries: Entries,
}

impl Open {
    fn empty(backing: Backing) -> Self {
        Self {
            backing,
            entries: Entries::new(),
        }
    }
}

/// Secrets in an AES-256-GCM file whose key is held by `K`. Opened on the first call
/// that needs a secret, so constructing it (and `status`) never prompts.
pub struct FileVault<K: KeySource> {
    path: PathBuf,
    keys: K,
    state: Mutex<Option<Open>>,
}

fn encode(entries: &Entries) -> Result<Zeroizing<Vec<u8>>, AppError> {
    let plain = PlainOut {
        version: PLAIN_VERSION,
        entries: entries
            .iter()
            .map(|(id, value)| (id.as_str(), value.as_str()))
            .collect(),
    };
    // Sized for the worst case up front, so the buffer never reallocates and leaves an
    // unwiped copy of the secrets behind.
    let content: usize = entries
        .iter()
        .map(|(id, value)| id.len() + value.len() + 1)
        .sum();
    let mut json = Zeroizing::new(Vec::with_capacity(64 + content * JSON_WORST_CASE_GROWTH));
    serde_json::to_writer(&mut *json, &plain)
        .map_err(|_| AppError::internal("Secret storage could not be written."))?;
    Ok(json)
}

fn decode(key: &[u8], file: &[u8]) -> Result<Entries, Unreadable> {
    let json = crypto::open(key, file).map_err(|_| Unreadable)?;
    let plain: PlainIn = serde_json::from_slice(&json).map_err(|_| Unreadable)?;
    let entries: Entries = plain
        .entries
        .into_iter()
        .map(|(id, value)| (id, Zeroizing::new(value)))
        .collect();
    if plain.version != PLAIN_VERSION {
        return Err(Unreadable);
    }
    Ok(entries)
}

fn apply_change(entries: &mut Entries, change: &SecretChange) {
    match change {
        SecretChange::Set { id, value } => {
            entries.insert(id.clone(), Zeroizing::new(value.clone()));
        }
        SecretChange::Delete { id } => {
            entries.remove(id);
        }
        SecretChange::Move { from, to } => {
            if let Some(value) = entries.remove(from) {
                entries.insert(to.clone(), value);
            }
        }
    }
}

fn validate(changes: &[SecretChange]) -> Result<(), AppError> {
    for change in changes {
        let (id, value_len) = match change {
            SecretChange::Set { id, value } => (id, value.len()),
            SecretChange::Delete { id } => (id, 0),
            SecretChange::Move { to, .. } => (to, 0),
        };
        if id.is_empty() {
            return Err(AppError::invalid_input("A secret needs a name."));
        }
        if value_len > MAX_VALUE_BYTES {
            return Err(AppError::invalid_input(
                "A secret value is larger than 64 KiB.",
            ));
        }
    }
    Ok(())
}

fn io_error(path: &Path, source: std::io::Error) -> AppError {
    let detail = format!("{}: {source}", path.display());
    AppError::from(source).with_detail(detail)
}

/// Replaces `target` through a temp file in the same directory and a rename. The file is
/// readable by its owner only.
fn write_private(target: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let parent = target
        .parent()
        .ok_or_else(|| AppError::internal("A file operation failed.").with_detail("no parent"))?;
    std::fs::create_dir_all(parent).map_err(|source| io_error(target, source))?;
    let mut temp = tempfile::Builder::new()
        .prefix(TEMP_PREFIX)
        .tempfile_in(parent)
        .map_err(|source| io_error(target, source))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|source| io_error(target, source))?;
    }
    temp.write_all(bytes)
        .and_then(|()| temp.as_file().sync_all())
        .map_err(|source| io_error(target, source))?;
    temp.persist(target)
        .map_err(|error| io_error(target, error.error))?;
    Ok(())
}

impl<K: KeySource> FileVault<K> {
    /// `path` is the vault file (`<app data>/secrets.vault`). Touches neither the file
    /// nor the key store.
    pub fn new(path: impl Into<PathBuf>, keys: K) -> Self {
        Self {
            path: path.into(),
            keys,
            state: Mutex::new(None),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Option<Open>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Something (a file or anything else) is at the vault path. An entry that cannot
    /// be inspected counts as present, so it is never overwritten.
    fn file_present(&self) -> bool {
        match std::fs::symlink_metadata(&self.path) {
            Ok(_) => true,
            Err(error) => error.kind() != std::io::ErrorKind::NotFound,
        }
    }

    /// The entries on disk; no file is an empty vault.
    fn read_entries(&self, key: &[u8]) -> Result<Entries, Unreadable> {
        if !self.file_present() {
            return Ok(Entries::new());
        }
        let meta = std::fs::symlink_metadata(&self.path).map_err(|_| Unreadable)?;
        if !meta.file_type().is_file() || meta.len() > MAX_FILE_BYTES as u64 {
            return Err(Unreadable);
        }
        let file = std::fs::read(&self.path).map_err(|_| Unreadable)?;
        decode(key, &file)
    }

    fn open(&self) -> Open {
        let stored = match self.keys.load() {
            Ok(stored) => stored,
            Err(error) => {
                tracing::warn!(?error, "secret storage: key store not usable, session only");
                return Open::empty(Backing::SessionOnly);
            }
        };
        let key = match stored {
            Some(key) => Zeroizing::new(key),
            None if self.file_present() => {
                tracing::warn!("secret storage: vault file without a key");
                return Open::empty(Backing::Damaged);
            }
            None => match self.create_key() {
                Some(key) => key,
                None => return Open::empty(Backing::SessionOnly),
            },
        };
        if key.len() != crypto::KEY_LEN {
            tracing::warn!("secret storage: the stored key is not a vault key");
            return Open::empty(Backing::Damaged);
        }
        match self.read_entries(&key) {
            Ok(entries) => Open {
                backing: Backing::Keychain(key),
                entries,
            },
            Err(Unreadable) => {
                tracing::warn!("secret storage: the vault file cannot be opened");
                Open::empty(Backing::Damaged)
            }
        }
    }

    fn create_key(&self) -> Option<Zeroizing<Vec<u8>>> {
        let key = crypto::generate_key()
            .map_err(|error| tracing::warn!(?error, "secret storage: no random key"))
            .ok()?;
        self.keys
            .store(&key)
            .map_err(|error| tracing::warn!(?error, "secret storage: key not stored"))
            .ok()?;
        Some(key)
    }

    fn persist(&self, key: &[u8], entries: &Entries) -> Result<(), AppError> {
        let json = encode(entries)?;
        if json.len() + crypto::OVERHEAD > MAX_FILE_BYTES {
            return Err(AppError::invalid_input(
                "Secret storage is full (5 MiB). Remove some secrets first.",
            ));
        }
        let file = crypto::seal(key, &json)
            .map_err(|_| AppError::internal("Secret storage could not be encrypted."))?;
        write_private(&self.path, &file)
    }

    /// Runs `change` on a copy of the entries and keeps the copy only if it could be
    /// stored. In keychain mode the file is read again first and written at most once.
    fn update(&self, change: &dyn Fn(&mut Entries)) -> Result<(), AppError> {
        let mut guard = self.lock();
        let open = guard.get_or_insert_with(|| self.open());
        if let Backing::Keychain(key) = &open.backing {
            if let Ok(on_disk) = self.read_entries(key) {
                let mut next = on_disk.clone();
                change(&mut next);
                if next != on_disk {
                    self.persist(key, &next)?;
                }
                open.entries = next;
                return Ok(());
            }
            tracing::warn!("secret storage: the vault file can no longer be opened");
            open.backing = Backing::Damaged;
        }
        let mut next = open.entries.clone();
        change(&mut next);
        open.entries = next;
        Ok(())
    }
}

impl<K: KeySource> SecretStore for FileVault<K> {
    fn get(&self, id: &str) -> Result<Option<String>, AppError> {
        let mut guard = self.lock();
        let open = guard.get_or_insert_with(|| self.open());
        Ok(open.entries.get(id).map(|value| value.as_str().to_string()))
    }

    fn has(&self, id: &str) -> Result<bool, AppError> {
        let mut guard = self.lock();
        let open = guard.get_or_insert_with(|| self.open());
        Ok(open.entries.contains_key(id))
    }

    fn apply(&self, changes: &[SecretChange]) -> Result<(), AppError> {
        validate(changes)?;
        self.update(&|entries| {
            for change in changes {
                apply_change(entries, change);
            }
        })
    }

    fn delete_prefix(&self, prefix: &str) -> Result<(), AppError> {
        self.update(&|entries| entries.retain(|id, _| !id.starts_with(prefix)))
    }

    /// The mode found by the first call that needed a secret; `keychain` before that.
    fn status(&self) -> SecretStorageStatus {
        let mode = match self.lock().as_ref().map(|open| &open.backing) {
            None | Some(Backing::Keychain(_)) => SecretStorageMode::Keychain,
            Some(Backing::SessionOnly) => SecretStorageMode::SessionOnly,
            Some(Backing::Damaged) => SecretStorageMode::Damaged,
        };
        status_of(mode)
    }

    /// Deletes the file, then the key, then opens again so `status` is current.
    fn reset(&self) -> Result<(), AppError> {
        let mut guard = self.lock();
        match std::fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(&self.path, error)),
        }
        // A key left behind without a file is harmless: it opens as an empty vault.
        if let Err(error) = self.keys.delete() {
            tracing::warn!(?error, "secret storage: the key could not be deleted");
        }
        *guard = Some(self.open());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; crypto::KEY_LEN] = [3; crypto::KEY_LEN];

    fn entries(pairs: &[(&str, &str)]) -> Entries {
        pairs
            .iter()
            .map(|(id, value)| (id.to_string(), Zeroizing::new(value.to_string())))
            .collect()
    }

    #[test]
    fn plaintext_is_the_versioned_json_of_the_plan() {
        let json = encode(&entries(&[("g/b", "2"), ("g/a", "1")])).unwrap();

        assert_eq!(
            json.as_slice(),
            br#"{"version":1,"entries":{"g/a":"1","g/b":"2"}}"#
        );
    }

    #[test]
    fn encode_then_decode_keeps_awkward_values() {
        let original = entries(&[("e/c/dev/na/me", "line\nbreak \u{1} \"quoted\" é")]);
        let file = crypto::seal(&KEY, &encode(&original).unwrap()).unwrap();

        assert!(decode(&KEY, &file).ok() == Some(original));
    }

    #[test]
    fn worst_case_json_fits_the_reserved_buffer() {
        let original = entries(&[("\u{1}\u{2}", &"\u{1}".repeat(1000))]);

        let json = encode(&original).unwrap();

        assert!(json.len() <= 64 + (2 + 1000 + 1) * JSON_WORST_CASE_GROWTH);
    }

    #[test]
    fn authentic_file_with_unknown_content_is_unreadable() {
        let sealed = |plain: &[u8]| crypto::seal(&KEY, plain).unwrap();

        assert!(decode(&KEY, &sealed(br#"{"version":2,"entries":{}}"#)).is_err());
        assert!(decode(&KEY, &sealed(br#"{"version":1}"#)).is_err());
        assert!(decode(&KEY, &sealed(b"not json")).is_err());
        assert!(decode(&KEY, &sealed(br#"{"version":1,"entries":{}}"#)).is_ok());
    }
}
