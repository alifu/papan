//! The vault key in the OS keychain (plan 04 §4.1). Kept small: CI cannot cover it; the
//! `#[ignore]` test in `tests/vault.rs` is run by hand on each OS.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use keyring::{Entry, Error};
use zeroize::{Zeroize, Zeroizing};

use super::keys::{KeyError, KeySource};

pub const SERVICE: &str = "com.papan-lifux.app";
pub const ACCOUNT: &str = "secret-vault-key-v1";

/// One keychain item holding base64 of the vault key.
pub struct OsKeys {
    account: String,
}

impl OsKeys {
    pub fn new() -> Self {
        Self::for_account(ACCOUNT)
    }

    /// Another item of the same service, so a test never touches the real key.
    pub fn for_account(account: impl Into<String>) -> Self {
        Self {
            account: account.into(),
        }
    }

    fn entry(&self) -> Result<Entry, KeyError> {
        Entry::new(SERVICE, &self.account).map_err(key_error)
    }
}

impl Default for OsKeys {
    fn default() -> Self {
        Self::new()
    }
}

/// Only platform errors are formatted: the other variants can carry item contents.
fn key_error(error: Error) -> KeyError {
    match error {
        Error::NoStorageAccess(source) => KeyError::Denied(source.to_string()),
        Error::PlatformFailure(source) => KeyError::Unavailable(source.to_string()),
        _ => KeyError::Unavailable("the keychain item could not be used".to_string()),
    }
}

impl KeySource for OsKeys {
    /// An item that is not base64 is returned as an empty key, which the vault reports
    /// as damaged instead of silently replacing it.
    fn load(&self) -> Result<Option<Vec<u8>>, KeyError> {
        match self.entry()?.get_password() {
            Ok(encoded) => {
                let encoded = Zeroizing::new(encoded);
                Ok(Some(
                    STANDARD.decode(encoded.as_bytes()).unwrap_or_default(),
                ))
            }
            Err(Error::NoEntry) => Ok(None),
            Err(Error::BadEncoding(mut bytes)) => {
                bytes.zeroize();
                Ok(Some(Vec::new()))
            }
            Err(error) => Err(key_error(error)),
        }
    }

    fn store(&self, key: &[u8]) -> Result<(), KeyError> {
        let encoded = Zeroizing::new(STANDARD.encode(key));
        self.entry()?.set_password(&encoded).map_err(key_error)
    }

    fn delete(&self) -> Result<(), KeyError> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(error) => Err(key_error(error)),
        }
    }
}
