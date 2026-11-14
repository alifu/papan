//! Cancel flags of running script chains by request id, so `cancel_request` stops a
//! script as well as the send (plan 05 §4.2).

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::script::{CancelFlag, StrayThreads};

#[derive(Debug, Default)]
pub struct ScriptFlags {
    flags: Mutex<HashMap<String, CancelFlag>>,
    /// Script threads abandoned by any send of this engine.
    strays: StrayThreads,
}

/// The flag of one send; leaves the registry when dropped, unless a later send under the
/// same id took its slot.
pub struct ScriptRun<'a> {
    flags: &'a ScriptFlags,
    id: String,
    pub flag: CancelFlag,
    pub strays: StrayThreads,
}

impl Drop for ScriptRun<'_> {
    fn drop(&mut self) {
        let mut flags = self.flags.lock();
        if flags
            .get(&self.id)
            .is_some_and(|flag| flag.is_same(&self.flag))
        {
            flags.remove(&self.id);
        }
    }
}

impl ScriptFlags {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, CancelFlag>> {
        self.flags.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Registers a fresh flag under `id` for the lifetime of the returned guard.
    pub fn register(&self, id: &str) -> ScriptRun<'_> {
        let flag = CancelFlag::new();
        self.lock().insert(id.to_string(), flag.clone());
        ScriptRun {
            flags: self,
            id: id.to_string(),
            flag,
            strays: self.strays.clone(),
        }
    }

    /// Raises the flag of `id`; an unknown id is a no-op.
    pub fn cancel(&self, id: &str) {
        if let Some(flag) = self.lock().get(id) {
            flag.cancel();
        }
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_raises_the_registered_flag_and_drop_unregisters() {
        let flags = ScriptFlags::default();
        let run = flags.register("a");

        flags.cancel("unknown");
        assert!(!run.flag.is_cancelled());
        flags.cancel("a");
        assert!(run.flag.is_cancelled());

        drop(run);
        assert!(flags.is_empty());
    }

    #[test]
    fn an_earlier_send_under_the_same_id_does_not_unregister_the_later_one() {
        let flags = ScriptFlags::default();
        let first = flags.register("a");
        let second = flags.register("a");

        drop(first);
        flags.cancel("a");

        assert!(second.flag.is_cancelled());
        drop(second);
        assert!(flags.is_empty());
    }
}
