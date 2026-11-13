//! Stored tokens (plan 03b §5.2, §15.1). One vault record per owner and configuration
//! key, `o/<owner>/<config_key>`, and a per-owner index `o/<owner>/index` that bounds
//! the records to [`MAX_RECORDS`]. Nothing else is written under `o/`.
//! Blocking: every function may open the vault.

use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::error::AppError;
use crate::store::types::RequestLocation;
use crate::vars::vault::{SecretChange, SecretStore, MAX_VALUE_BYTES};

/// Records kept per owner; the oldest is evicted beyond this.
pub const MAX_RECORDS: usize = 20;
const SCRATCH_OWNER: &str = "scratch";
const INDEX: &str = "index";
const KEY_CHARS: usize = 64;

/// "Read the index, write record + index + evictions" is one step per process.
static INDEX_LOCK: Mutex<()> = Mutex::new(());

/// Who a token belongs to: one collection (its machine-local id) or the scratch area.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Owner(String);

impl Owner {
    /// The owner of the request at `location`. A collection id is validated as a UUID
    /// before it becomes part of a vault id.
    pub fn of(location: &RequestLocation) -> Result<Self, AppError> {
        match location {
            RequestLocation::Scratch { .. } => Ok(Self(SCRATCH_OWNER.to_string())),
            RequestLocation::Collection { collection_id, .. } => {
                uuid::Uuid::parse_str(collection_id)
                    .ok()
                    .map(|id| id.as_hyphenated().to_string())
                    .filter(|canonical| canonical == collection_id)
                    .map(Self)
                    .ok_or_else(|| AppError::invalid_input("That collection id is not valid."))
            }
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The value of one record.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenRecord {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
    pub scope: String,
    /// The only place the refresh token is ever sent.
    pub token_url: String,
    pub saved_at_ms: u64,
    /// Whether the token endpoint's certificate was verified when this was issued. A
    /// record without the field is treated as verified, the stricter reading.
    #[serde(default = "verified")]
    pub verify_tls: bool,
}

fn verified() -> bool {
    true
}

impl std::fmt::Debug for TokenRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenRecord")
            .field("expires_at_ms", &self.expires_at_ms)
            .field("has_refresh_token", &self.refresh_token.is_some())
            .field("saved_at_ms", &self.saved_at_ms)
            .finish_non_exhaustive()
    }
}

impl Drop for TokenRecord {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct IndexEntry {
    key: String,
    saved_at_ms: u64,
}

fn valid_key(key: &str) -> Result<(), AppError> {
    let valid = key.len() == KEY_CHARS
        && key
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if valid {
        Ok(())
    } else {
        Err(AppError::invalid_input("That token key is not valid."))
    }
}

fn record_id(owner: &Owner, key: &str) -> String {
    format!("o/{}/{key}", owner.0)
}

fn index_id(owner: &Owner) -> String {
    format!("o/{}/{INDEX}", owner.0)
}

/// The index of `owner`; a missing or unreadable one reads as empty.
fn read_index(vault: &dyn SecretStore, owner: &Owner) -> Result<Vec<IndexEntry>, AppError> {
    let text = vault.get(&index_id(owner))?.map(Zeroizing::new);
    Ok(text
        .and_then(|text| serde_json::from_str::<Vec<IndexEntry>>(&text).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| valid_key(&entry.key).is_ok())
        .collect())
}

fn index_change(owner: &Owner, index: &[IndexEntry]) -> Result<SecretChange, AppError> {
    if index.is_empty() {
        return Ok(SecretChange::Delete {
            id: index_id(owner),
        });
    }
    let value = serde_json::to_string(index).map_err(|source| {
        AppError::internal("Could not store the token.").with_detail(source.to_string())
    })?;
    Ok(SecretChange::Set {
        id: index_id(owner),
        value,
    })
}

/// The stored record of `key` for `owner`. A record that cannot be read is absent.
pub fn load(
    vault: &dyn SecretStore,
    owner: &Owner,
    key: &str,
) -> Result<Option<TokenRecord>, AppError> {
    valid_key(key)?;
    let text = vault.get(&record_id(owner, key))?.map(Zeroizing::new);
    let record = text.and_then(|text| serde_json::from_str::<TokenRecord>(&text).ok());
    Ok(record.filter(|record| !record.access_token.is_empty()))
}

/// Stores `record` under `key`, evicting the owner's oldest records beyond
/// [`MAX_RECORDS`]. Record, index and evictions go in one `apply`.
pub fn save(
    vault: &dyn SecretStore,
    owner: &Owner,
    key: &str,
    record: &TokenRecord,
) -> Result<(), AppError> {
    valid_key(key)?;
    let value = Zeroizing::new(serde_json::to_string(record).map_err(|_| {
        // No source text: it could quote the token.
        AppError::internal("Could not store the token.")
    })?);
    if value.len() > MAX_VALUE_BYTES {
        return Err(AppError::invalid_input("The token is too large to store."));
    }
    let _guard = INDEX_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let mut index: Vec<IndexEntry> = read_index(vault, owner)?
        .into_iter()
        .filter(|entry| entry.key != key)
        .collect();
    index.push(IndexEntry {
        key: key.to_string(),
        saved_at_ms: record.saved_at_ms,
    });
    // Oldest first; the sort is stable, so equal times keep their order.
    index.sort_by_key(|entry| entry.saved_at_ms);
    let excess = index.len().saturating_sub(MAX_RECORDS);
    let evicted: Vec<IndexEntry> = index.drain(..excess).collect();
    let mut changes = vec![
        SecretChange::Set {
            id: record_id(owner, key),
            value: value.to_string(),
        },
        index_change(owner, &index)?,
    ];
    changes.extend(evicted.iter().map(|entry| SecretChange::Delete {
        id: record_id(owner, &entry.key),
    }));
    vault.apply(&changes)
}

/// Forgets the record of `key` for `owner`; the index goes with its last record.
pub fn clear(vault: &dyn SecretStore, owner: &Owner, key: &str) -> Result<(), AppError> {
    valid_key(key)?;
    let _guard = INDEX_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let index: Vec<IndexEntry> = read_index(vault, owner)?
        .into_iter()
        .filter(|entry| entry.key != key)
        .collect();
    vault.apply(&[
        SecretChange::Delete {
            id: record_id(owner, key),
        },
        index_change(owner, &index)?,
    ])
}

#[cfg(test)]
#[path = "tokens_tests.rs"]
mod tests;
