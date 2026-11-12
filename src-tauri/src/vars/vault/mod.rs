//! Secret storage (plan 04 §4). The trait is the contract the rest of the app uses;
//! `MemoryVault` backs tests outside this module and the session-only mode.

mod crypto;
mod file_vault;
mod keys;
mod os_keys;

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use super::types::{SecretStorageMode, SecretStorageStatus};
use crate::error::AppError;

pub use file_vault::{FileVault, MAX_FILE_BYTES, MAX_VALUE_BYTES};
pub use keys::{KeyError, KeySource, MemoryKeys};
pub use os_keys::{OsKeys, ACCOUNT, SERVICE};

/// One change to the stored secrets. Ids follow §4.1 (`g/..`, `c/..`, `e/..`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretChange {
    Set { id: String, value: String },
    Delete { id: String },
    Move { from: String, to: String },
}

pub trait SecretStore: Send + Sync {
    fn get(&self, id: &str) -> Result<Option<String>, AppError>;
    fn has(&self, id: &str) -> Result<bool, AppError>;
    /// Applies all changes in one atomic write.
    fn apply(&self, changes: &[SecretChange]) -> Result<(), AppError>;
    fn delete_prefix(&self, prefix: &str) -> Result<(), AppError>;
    fn status(&self) -> SecretStorageStatus;
    /// Deletes the vault file and its key.
    fn reset(&self) -> Result<(), AppError>;
}

/// The status shown for a storage mode, with its warning text (plan 04 §4.3).
pub(crate) fn status_of(mode: SecretStorageMode) -> SecretStorageStatus {
    let message = match mode {
        SecretStorageMode::Keychain => None,
        SecretStorageMode::SessionOnly => {
            Some("No system keychain. Secret values are kept until you quit.".to_string())
        }
        SecretStorageMode::Damaged => Some(
            "Secret storage cannot be read. Secret values are kept until you quit.".to_string(),
        ),
    };
    SecretStorageStatus { mode, message }
}

/// Secrets held in memory only, reported with a fixed storage mode.
#[derive(Debug)]
pub struct MemoryVault {
    mode: SecretStorageMode,
    entries: Mutex<HashMap<String, String>>,
}

impl MemoryVault {
    pub fn new(mode: SecretStorageMode) -> Self {
        Self {
            mode,
            entries: Mutex::new(HashMap::new()),
        }
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<String, String>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for MemoryVault {
    fn default() -> Self {
        Self::new(SecretStorageMode::Keychain)
    }
}

impl SecretStore for MemoryVault {
    fn get(&self, id: &str) -> Result<Option<String>, AppError> {
        Ok(self.entries().get(id).cloned())
    }

    fn has(&self, id: &str) -> Result<bool, AppError> {
        Ok(self.entries().contains_key(id))
    }

    fn apply(&self, changes: &[SecretChange]) -> Result<(), AppError> {
        let mut entries = self.entries();
        for change in changes {
            match change {
                SecretChange::Set { id, value } => {
                    entries.insert(id.clone(), value.clone());
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
        Ok(())
    }

    fn delete_prefix(&self, prefix: &str) -> Result<(), AppError> {
        self.entries().retain(|id, _| !id.starts_with(prefix));
        Ok(())
    }

    fn status(&self) -> SecretStorageStatus {
        status_of(self.mode)
    }

    fn reset(&self) -> Result<(), AppError> {
        self.entries().clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(id: &str, value: &str) -> SecretChange {
        SecretChange::Set {
            id: id.to_string(),
            value: value.to_string(),
        }
    }

    #[test]
    fn memory_vault_applies_set_move_and_delete() {
        let vault = MemoryVault::default();

        vault.apply(&[set("g/a", "1"), set("g/b", "2")]).unwrap();
        vault
            .apply(&[
                SecretChange::Move {
                    from: "g/a".to_string(),
                    to: "g/c".to_string(),
                },
                SecretChange::Delete {
                    id: "g/b".to_string(),
                },
            ])
            .unwrap();

        assert_eq!(vault.get("g/c").unwrap().as_deref(), Some("1"));
        assert!(!vault.has("g/a").unwrap());
        assert!(!vault.has("g/b").unwrap());
    }

    #[test]
    fn memory_vault_deletes_by_prefix_and_resets() {
        let vault = MemoryVault::new(SecretStorageMode::SessionOnly);
        vault
            .apply(&[
                set("e/c1/dev/a", "1"),
                set("e/c1/prod/a", "2"),
                set("g/a", "3"),
            ])
            .unwrap();

        vault.delete_prefix("e/c1/dev/").unwrap();
        assert!(!vault.has("e/c1/dev/a").unwrap());
        assert!(vault.has("e/c1/prod/a").unwrap());

        vault.reset().unwrap();
        assert!(!vault.has("g/a").unwrap());
        assert_eq!(vault.status().mode, SecretStorageMode::SessionOnly);
        assert!(vault.status().message.is_some());
    }
}
