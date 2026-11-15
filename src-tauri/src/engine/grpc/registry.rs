//! The calls of a [`super::GrpcEngine`] by id. Every change a start or a call task makes
//! is guarded by its attempt number, so a superseded attempt can never touch the entry
//! of a newer one.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use prost_reflect::{DynamicMessage, MessageDescriptor};
use tokio::sync::{mpsc, oneshot, watch};

use super::types::GrpcMethodKind;
use super::{MAX_CALLS, MAX_CLOSED_LOGS};
use crate::engine::ws::log::WsLog;
use crate::engine::ws::messages::Masker;
use crate::engine::ws::session::SharedLog;
use crate::error::AppError;

pub(super) struct Running {
    pub cancel: Option<oneshot::Sender<()>>,
    /// The request stream of a client or bidi call; `None` once it is half-closed.
    pub tx: Option<mpsc::Sender<DynamicMessage>>,
    pub input: MessageDescriptor,
    pub kind: GrpcMethodKind,
    pub masker: Masker,
}

enum State {
    /// Preparing; the sender is gone once a cancel took it.
    Connecting(Option<oneshot::Sender<()>>),
    Running(Box<Running>),
    Finished,
}

struct Entry {
    attempt: u32,
    log: SharedLog,
    state: State,
    /// Turns `true` when the task of the last installed call has reported `finished`.
    done: Option<watch::Receiver<bool>>,
}

impl Entry {
    fn is_live(&self) -> bool {
        !matches!(self.state, State::Finished)
    }

    fn signal_cancel(&mut self) {
        let sender = match &mut self.state {
            State::Connecting(cancel) => cancel.take(),
            State::Running(running) => running.cancel.take(),
            State::Finished => None,
        };
        if let Some(sender) = sender {
            // The receiver is gone when the call is already ending.
            let _ = sender.send(());
        }
    }
}

#[derive(Default)]
pub(super) struct Registry {
    entries: HashMap<String, Entry>,
    /// The last attempt number of every id. Kept apart from the entries, which are
    /// evicted, so a number is never handed out twice.
    attempts: HashMap<String, u32>,
    closed: VecDeque<String>,
}

impl Registry {
    pub fn live(&self) -> usize {
        self.entries.values().filter(|e| e.is_live()).count()
    }

    /// Claims `id` for a new attempt. What ran or was starting under it is cancelled;
    /// the returned receiver tells when the previous call has reported `finished`.
    pub fn reserve(
        &mut self,
        id: &str,
        cancel: oneshot::Sender<()>,
    ) -> Result<(u32, Option<watch::Receiver<bool>>), AppError> {
        let replaces_live = self.entries.get(id).is_some_and(Entry::is_live);
        if !replaces_live && self.live() >= MAX_CALLS {
            return Err(AppError::invalid_input(format!(
                "At most {MAX_CALLS} gRPC calls can run at once."
            )));
        }
        self.closed.retain(|closed| closed != id);
        let attempt = {
            let last = self.attempts.entry(id.to_string()).or_insert(0);
            *last += 1;
            *last
        };
        let Some(entry) = self.entries.get_mut(id) else {
            let entry = Entry {
                attempt,
                log: Arc::new(Mutex::new(WsLog::default())),
                state: State::Connecting(Some(cancel)),
                done: None,
            };
            self.entries.insert(id.to_string(), entry);
            return Ok((attempt, None));
        };
        entry.signal_cancel();
        entry.attempt = attempt;
        entry.state = State::Connecting(Some(cancel));
        Ok((attempt, entry.done.clone()))
    }

    /// Makes attempt `attempt` of `id` the running call. `false` when a cancel took its
    /// handle or a newer attempt claimed the id: the call must not start.
    pub fn install(
        &mut self,
        id: &str,
        attempt: u32,
        running: Running,
        log: SharedLog,
        done: watch::Receiver<bool>,
    ) -> bool {
        let Some(entry) = self.entries.get_mut(id) else {
            return false;
        };
        let expected =
            entry.attempt == attempt && matches!(entry.state, State::Connecting(Some(_)));
        if !expected {
            return false;
        }
        entry.state = State::Running(Box::new(running));
        entry.log = log;
        entry.done = Some(done);
        true
    }

    /// A start that failed or was cancelled before its call was installed.
    pub fn abandon(&mut self, id: &str, attempt: u32) {
        let is_ours =
            |entry: &Entry| entry.attempt == attempt && matches!(entry.state, State::Connecting(_));
        if self.entries.get(id).is_some_and(is_ours) {
            self.mark_finished(id);
        }
    }

    /// The task of attempt `attempt` has reported `finished`.
    pub fn finish(&mut self, id: &str, attempt: u32) {
        let is_ours =
            |entry: &Entry| entry.attempt == attempt && matches!(entry.state, State::Running(_));
        if self.entries.get(id).is_some_and(is_ours) {
            self.mark_finished(id);
        }
    }

    fn mark_finished(&mut self, id: &str) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.state = State::Finished;
        }
        self.closed.retain(|closed| closed != id);
        self.closed.push_back(id.to_string());
        while self.closed.len() > MAX_CLOSED_LOGS {
            if let Some(oldest) = self.closed.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }

    pub fn cancel(&mut self, id: &str) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.signal_cancel();
        }
    }

    pub fn running(&self, id: &str) -> Option<(&Running, SharedLog)> {
        let entry = self.entries.get(id)?;
        match &entry.state {
            State::Running(running) => Some((running, Arc::clone(&entry.log))),
            State::Connecting(_) | State::Finished => None,
        }
    }

    pub fn running_mut(&mut self, id: &str) -> Option<&mut Running> {
        match &mut self.entries.get_mut(id)?.state {
            State::Running(running) => Some(running),
            State::Connecting(_) | State::Finished => None,
        }
    }

    pub fn log_of(&self, id: &str) -> Option<SharedLog> {
        self.entries.get(id).map(|entry| Arc::clone(&entry.log))
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}
