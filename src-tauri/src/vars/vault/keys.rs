//! Where the vault key lives (plan 04 §4.2). `OsKeys` is the real one; `MemoryKeys`
//! backs every test, so no test touches the OS keychain.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use zeroize::Zeroizing;

/// Why the key store could not be used. The text is for logs and never holds key bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    /// No key store on this machine, or it failed.
    Unavailable(String),
    /// The key store exists but refused access.
    Denied(String),
}

pub trait KeySource: Send + Sync {
    /// The stored key, or `None` when there is none.
    fn load(&self) -> Result<Option<Vec<u8>>, KeyError>;
    fn store(&self, key: &[u8]) -> Result<(), KeyError>;
    /// Removing a key that is not there succeeds.
    fn delete(&self) -> Result<(), KeyError>;
}

#[derive(Default)]
struct MemoryKeysInner {
    key: Option<Zeroizing<Vec<u8>>>,
    failure: Option<KeyError>,
}

/// A key held in memory. Clones share the same key, so a test can keep a handle to
/// inspect or break the key store after giving one to a vault.
#[derive(Clone, Default)]
pub struct MemoryKeys {
    inner: Arc<Mutex<MemoryKeysInner>>,
}

impl MemoryKeys {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_key(key: &[u8]) -> Self {
        let keys = Self::default();
        keys.inner().key = Some(Zeroizing::new(key.to_vec()));
        keys
    }

    /// A key store where every call fails with `error`.
    pub fn failing(error: KeyError) -> Self {
        let keys = Self::default();
        keys.set_failure(Some(error));
        keys
    }

    /// Makes every later call fail with `failure`, or work again with `None`.
    pub fn set_failure(&self, failure: Option<KeyError>) {
        self.inner().failure = failure;
    }

    /// The key currently held, whatever the failure setting.
    pub fn current(&self) -> Option<Vec<u8>> {
        self.inner().key.as_ref().map(|key| key.to_vec())
    }

    fn inner(&self) -> MutexGuard<'_, MemoryKeysInner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn working(&self) -> Result<MutexGuard<'_, MemoryKeysInner>, KeyError> {
        let inner = self.inner();
        match &inner.failure {
            Some(error) => Err(error.clone()),
            None => Ok(inner),
        }
    }
}

impl KeySource for MemoryKeys {
    fn load(&self) -> Result<Option<Vec<u8>>, KeyError> {
        Ok(self.working()?.key.as_ref().map(|key| key.to_vec()))
    }

    fn store(&self, key: &[u8]) -> Result<(), KeyError> {
        self.working()?.key = Some(Zeroizing::new(key.to_vec()));
        Ok(())
    }

    fn delete(&self) -> Result<(), KeyError> {
        self.working()?.key = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_keys_store_load_and_delete() {
        let keys = MemoryKeys::new();
        assert_eq!(keys.load().unwrap(), None);

        keys.store(&[1, 2, 3]).unwrap();
        assert_eq!(keys.load().unwrap(), Some(vec![1, 2, 3]));

        keys.delete().unwrap();
        keys.delete().unwrap();
        assert_eq!(keys.load().unwrap(), None);
    }

    #[test]
    fn clones_share_one_key() {
        let keys = MemoryKeys::with_key(&[9; 4]);
        let handle = keys.clone();

        keys.store(&[5; 4]).unwrap();

        assert_eq!(handle.current(), Some(vec![5; 4]));
    }

    #[test]
    fn a_failing_store_fails_every_call_and_keeps_its_key() {
        let keys = MemoryKeys::with_key(&[9; 4]);
        let denied = KeyError::Denied("locked".to_string());
        keys.set_failure(Some(denied.clone()));

        assert_eq!(keys.load().unwrap_err(), denied);
        assert_eq!(keys.store(&[1]).unwrap_err(), denied);
        assert_eq!(keys.delete().unwrap_err(), denied);
        assert_eq!(keys.current(), Some(vec![9; 4]));

        keys.set_failure(None);
        assert_eq!(keys.load().unwrap(), Some(vec![9; 4]));

        let unavailable = MemoryKeys::failing(KeyError::Unavailable("none".to_string()));
        assert!(matches!(unavailable.load(), Err(KeyError::Unavailable(_))));
    }
}
