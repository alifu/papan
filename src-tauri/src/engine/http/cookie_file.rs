//! Persisted cookie jars (plan 03b §6.5, §15.5): `<app data>/cookies/<jar>.jar`, sealed
//! with a key that lives in the vault as `n/cookie-key`.
//!
//! Only unexpired persistent cookies are written. Without a usable keychain nothing is
//! read or written. The vault is opened only when there is something to read or write.
//! The caller decides which jars come here (the verified jar only) and when.

use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use reqwest_cookie_store::CookieStore;
use zeroize::Zeroizing;

use crate::error::AppError;
use crate::vars::types::SecretStorageMode;
use crate::vars::vault::{sealed, SecretChange, SecretStore};

/// A jar whose JSON is larger than this is not written.
pub const MAX_JAR_BYTES: usize = 4 * 1024 * 1024;

const MAGIC: &[u8; 8] = b"PAPANJAR";
const KEY_ID: &str = "n/cookie-key";
const SCRATCH: &str = "scratch";
const COLLECTION_PREFIX: &str = "collection-";
const EXTENSION: &str = "jar";

/// The jar's file, or `None` unless `jar_name` is `scratch` or `collection-<uuid>` with
/// the id in the lowercase hyphenated form.
fn jar_path(dir: &Path, jar_name: &str) -> Option<PathBuf> {
    let known = jar_name == SCRATCH
        || jar_name
            .strip_prefix(COLLECTION_PREFIX)
            .is_some_and(is_canonical_uuid);
    known.then(|| dir.join(format!("{jar_name}.{EXTENSION}")))
}

fn is_canonical_uuid(text: &str) -> bool {
    uuid::Uuid::parse_str(text).is_ok_and(|id| id.as_hyphenated().to_string() == text)
}

fn invalid_name() -> AppError {
    AppError::invalid_input("The cookie jar name is not valid.")
}

/// The stored cookie key. `None` when the keychain is not in use or no valid key is stored.
fn stored_key(vault: &dyn SecretStore) -> Result<Option<Zeroizing<Vec<u8>>>, AppError> {
    // `status()` is only right after a call that opened the vault (§15.1).
    let stored = vault.get(KEY_ID)?.map(Zeroizing::new);
    if vault.status().mode != SecretStorageMode::Keychain {
        return Ok(None);
    }
    Ok(stored
        .and_then(|text| STANDARD.decode(text.as_bytes()).ok().map(Zeroizing::new))
        .filter(|key| key.len() == sealed::KEY_LEN))
}

/// The stored key, or a new one written to the vault. `None` without a keychain.
fn key_for_save(vault: &dyn SecretStore) -> Result<Option<Zeroizing<Vec<u8>>>, AppError> {
    if let Some(key) = stored_key(vault)? {
        return Ok(Some(key));
    }
    if vault.status().mode != SecretStorageMode::Keychain {
        return Ok(None);
    }
    let key = sealed::generate_key()
        .map_err(|_| AppError::internal("Could not create the cookie key."))?;
    let encoded = Zeroizing::new(STANDARD.encode(key.as_slice()));
    vault.apply(&[SecretChange::Set {
        id: KEY_ID.to_string(),
        value: encoded.as_str().to_string(),
    }])?;
    // The write itself can show that the keychain is unusable.
    Ok((vault.status().mode == SecretStorageMode::Keychain).then_some(key))
}

/// A regular file of a plausible size, read whole.
fn read_jar(path: &Path) -> Option<Vec<u8>> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    let max = (MAX_JAR_BYTES + sealed::OVERHEAD) as u64;
    if !meta.file_type().is_file() || meta.len() > max {
        tracing::warn!("cookie jar file ignored: not a regular file or too large");
        return None;
    }
    std::fs::read(path).ok()
}

/// The sealed text starts with the jar name and a newline, so a file copied over another
/// jar's does not open there.
fn bound(jar_name: &str, json: &[u8]) -> Zeroizing<Vec<u8>> {
    Zeroizing::new([jar_name.as_bytes(), b"\n", json].concat())
}

fn decode(jar_name: &str, plain: &[u8]) -> Option<CookieStore> {
    let json = plain
        .strip_prefix(jar_name.as_bytes())?
        .strip_prefix(b"\n")?;
    let cookies: Vec<_> = serde_json::from_slice(json).ok()?;
    CookieStore::from_cookies(
        cookies.into_iter().map(Ok::<_, std::convert::Infallible>),
        false,
    )
    .ok()
}

/// The persisted cookies of a jar. `Ok(None)` when there is no file or it can never be
/// opened (damaged, another jar's, sealed with a key that is gone); the next `save`
/// replaces such a file. An error when the key could not be read now (the vault failed,
/// the keychain was denied or is unavailable): the file is kept and must not be saved over.
pub fn load(
    dir: &Path,
    jar_name: &str,
    vault: &dyn SecretStore,
) -> Result<Option<CookieStore>, AppError> {
    let path = jar_path(dir, jar_name).ok_or_else(invalid_name)?;
    let Some(file) = read_jar(&path) else {
        return Ok(None);
    };
    let stored = vault.get(KEY_ID)?.map(Zeroizing::new);
    if vault.status().mode != SecretStorageMode::Keychain {
        return Err(AppError::internal(
            "The cookie jar cannot be read: secret storage is not available.",
        ));
    }
    let store = stored
        .and_then(|text| STANDARD.decode(text.as_bytes()).ok().map(Zeroizing::new))
        .filter(|key| key.len() == sealed::KEY_LEN)
        .and_then(|key| sealed::open(MAGIC, &key, &file).ok())
        .and_then(|plain| decode(jar_name, &plain));
    if store.is_none() {
        tracing::warn!(jar = jar_name, "cookie jar file ignored: cannot be opened");
    }
    Ok(store)
}

/// What a save does to the jar's file, decided before it is done.
#[derive(Debug)]
pub enum JarWrite {
    /// No keychain or over the size limit: the file is left as it is.
    Nothing,
    Remove(PathBuf),
    Write(PathBuf, Vec<u8>),
}

impl JarWrite {
    pub fn commit(self) -> Result<(), AppError> {
        match self {
            Self::Nothing => Ok(()),
            Self::Remove(path) => remove(&path),
            Self::Write(path, file) => sealed::write_private(&path, &file),
        }
    }
}

/// Seals the jar's unexpired persistent cookies (this may open the vault); with none, the
/// file is to be removed. Without a keychain nothing is written.
pub fn prepare(
    dir: &Path,
    jar_name: &str,
    vault: &dyn SecretStore,
    store: &CookieStore,
) -> Result<JarWrite, AppError> {
    let path = jar_path(dir, jar_name).ok_or_else(invalid_name)?;
    let cookies: Vec<_> = store
        .iter_unexpired()
        .filter(|cookie| cookie.is_persistent())
        .collect();
    if cookies.is_empty() {
        return Ok(JarWrite::Remove(path));
    }
    let json = Zeroizing::new(serde_json::to_vec(&cookies).map_err(|_| {
        // The serde error can quote a cookie; it is not kept.
        AppError::internal("Could not save the cookies.")
    })?);
    let plain = bound(jar_name, &json);
    if plain.len() > MAX_JAR_BYTES {
        tracing::warn!(jar = jar_name, "cookie jar not saved: over 4 MiB");
        return Ok(JarWrite::Nothing);
    }
    let Some(key) = key_for_save(vault)? else {
        return Ok(JarWrite::Nothing);
    };
    let file = sealed::seal(MAGIC, &key, &plain)
        .map_err(|_| AppError::internal("Could not save the cookies."))?;
    Ok(JarWrite::Write(path, file))
}

/// Writes the jar's unexpired persistent cookies; with none, removes the file. Without a
/// keychain nothing is written and the cookies last for the session.
pub fn save(
    dir: &Path,
    jar_name: &str,
    vault: &dyn SecretStore,
    store: &CookieStore,
) -> Result<(), AppError> {
    prepare(dir, jar_name, vault, store)?.commit()
}

/// Removes the jar's file. A file that is not there is not an error.
pub fn delete(dir: &Path, jar_name: &str) -> Result<(), AppError> {
    remove(&jar_path(dir, jar_name).ok_or_else(invalid_name)?)
}

fn remove(path: &Path) -> Result<(), AppError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(AppError::from(source)),
    }
}
