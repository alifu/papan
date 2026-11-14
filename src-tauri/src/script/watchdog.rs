//! Host-side watchdog for a script chain.
//!
//! The engine's interrupt handler only runs between bytecode instructions, so a loop
//! inside one native call (`Array.prototype.map` over a bound built-in, a huge string
//! search) ignores both the deadline and Cancel. The chain therefore runs on a detached
//! thread and the caller waits for it only until the deadline, or the cancel, plus
//! [`WATCHDOG_GRACE`]. After that the thread is abandoned: it owns its copy of the
//! context, its own stop flag (raised here, so it ends at its next interrupt check) and
//! the sending half of a one-shot channel nobody reads any more, so nothing it does later
//! reaches the caller. A thread cannot be killed, so an abandoned one keeps its core and
//! its heap (bounded by `Limits::memory_bytes`) until its native call returns;
//! [`StrayThreads`] counts them and new chains are refused at [`MAX_STRAY_THREADS`].

use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::chain::Script;
use super::pm::HostState;
use super::report::{ScriptError, ScriptErrorKind};
use super::sandbox::{run_chain, CancelFlag, Limits};

/// How long a chain may take to stop by itself once it should, before it is abandoned.
pub const WATCHDOG_GRACE: Duration = Duration::from_millis(250);
/// Abandoned threads that may still be running before new chains are refused.
pub const MAX_STRAY_THREADS: usize = 4;
/// Stack kept free for the engine's own frames, on top of `Limits::stack_bytes`.
const STACK_HEADROOM_BYTES: usize = 1536 * 1024;
const POLL: Duration = Duration::from_millis(10);

const RUNNING: u8 = 0;
const FINISHED: u8 = 1;
const ABANDONED: u8 = 2;

/// Abandoned script threads that have not ended yet. Share one between engines to cap
/// them together.
#[derive(Debug, Clone, Default)]
pub struct StrayThreads(Arc<AtomicUsize>);

impl StrayThreads {
    pub fn count(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }

    pub fn is_full(&self) -> bool {
        self.count() >= MAX_STRAY_THREADS
    }
}

/// Lives on the script thread; leaves the stray count when an abandoned thread ends.
struct Tracker {
    state: Arc<AtomicU8>,
    strays: StrayThreads,
}

impl Drop for Tracker {
    fn drop(&mut self) {
        if self.state.swap(FINISHED, Ordering::SeqCst) == ABANDONED {
            self.strays.0.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// How a watched chain ended.
pub(super) enum Watched {
    /// The chain ended by itself: the host state it left and its error, if any.
    Finished(Box<HostState>, Option<ScriptError>),
    /// The chain did not stop in time and was abandoned.
    Abandoned(ScriptErrorKind),
    /// The thread could not start, or panicked.
    Failed,
}

/// Runs the chain on its own thread and waits for it, at most until `host.deadline` or
/// `cancel`, plus the grace.
pub(super) fn watch(
    host: HostState,
    scripts: Vec<Script>,
    limits: &Limits,
    cancel: &CancelFlag,
    strays: &StrayThreads,
) -> Watched {
    let deadline = host.deadline;
    let stop = CancelFlag::new();
    let state = Arc::new(AtomicU8::new(RUNNING));
    let (done, result) = sync_channel(1);
    let spawned = std::thread::Builder::new()
        .name("papan-script".into())
        .stack_size(limits.stack_bytes + STACK_HEADROOM_BYTES)
        .spawn({
            let tracker = Tracker {
                state: Arc::clone(&state),
                strays: strays.clone(),
            };
            let (limits, stop) = (limits.clone(), stop.clone());
            move || {
                let _tracker = tracker;
                // The receiver is gone once the chain was abandoned; nothing to do then.
                let _ = done.send(run_chain(host, &scripts, &limits, &stop));
            }
        });
    if let Err(error) = spawned {
        tracing::error!(%error, "could not start the script thread");
        return Watched::Failed;
    }

    let mut stopping: Option<(ScriptErrorKind, Instant)> = None;
    loop {
        match result.recv_timeout(POLL) {
            Ok((host, error)) => return Watched::Finished(Box::new(host), error),
            Err(RecvTimeoutError::Disconnected) => {
                tracing::error!("the script engine panicked");
                return Watched::Failed;
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        match stopping {
            Some((kind, give_up)) if now >= give_up => {
                stop.cancel();
                let abandoned =
                    state.compare_exchange(RUNNING, ABANDONED, Ordering::SeqCst, Ordering::SeqCst);
                if abandoned.is_ok() {
                    strays.0.fetch_add(1, Ordering::SeqCst);
                }
                tracing::warn!(?kind, "a script did not stop in time and was abandoned");
                return Watched::Abandoned(kind);
            }
            Some(_) => {}
            None if cancel.is_cancelled() => {
                stop.cancel();
                stopping = Some((ScriptErrorKind::Cancelled, now + WATCHDOG_GRACE));
            }
            None if now >= deadline => {
                stopping = Some((ScriptErrorKind::Timeout, now + WATCHDOG_GRACE));
            }
            None => {}
        }
    }
}
