//! Where a connection reports to (plan 07 §5). The session knows only [`WsInbound`], so
//! the Tauri event bridge and, in M6b, a `graphql-transport-ws` driver are interchangeable.
//! [`BatchingSink`] turns the stream of messages into at most one event per 50 ms.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super::types::{WsCloseBy, WsEvent, WsEventKind, WsMessage, WsOpened};
use crate::error::AppError;

pub const BATCH_INTERVAL: Duration = Duration::from_millis(50);
pub const BATCH_ITEMS: usize = 100;

/// Receives what happens on one connection. Called from the session task: keep it quick.
pub trait WsInbound: Send + Sync {
    fn opened(&self, opened: &WsOpened);
    /// A received frame, as logged, with its raw payload (not truncated, not masked).
    fn message(&self, message: &WsMessage, raw: &[u8]);
    fn closed(&self, code: Option<u32>, reason: &str, by: WsCloseBy, error: Option<&AppError>);
}

type Emit = Arc<dyn Fn(WsEvent) + Send + Sync>;

#[derive(Default)]
struct Pending {
    items: Vec<WsMessage>,
    timer_armed: bool,
}

/// Emits [`WsEvent`]s through `emit`; messages are batched (50 ms or 100 items), and
/// every other event flushes the pending batch first, so order is kept.
pub struct BatchingSink {
    connection_id: String,
    emit: Emit,
    pending: Arc<Mutex<Pending>>,
}

fn lock(pending: &Mutex<Pending>) -> MutexGuard<'_, Pending> {
    pending.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Emits the pending messages. The caller holds the lock for the whole emit, so no
/// later event can overtake the batch.
fn flush_locked(connection_id: &str, emit: &Emit, pending: &mut Pending) {
    let items = std::mem::take(&mut pending.items);
    if items.is_empty() {
        return;
    }
    emit(WsEvent {
        connection_id: connection_id.to_string(),
        kind: WsEventKind::Messages { items },
    });
}

fn flush(connection_id: &str, emit: &Emit, pending: &Mutex<Pending>) {
    flush_locked(connection_id, emit, &mut lock(pending));
}

impl BatchingSink {
    pub fn new(connection_id: &str, emit: impl Fn(WsEvent) + Send + Sync + 'static) -> Self {
        Self {
            connection_id: connection_id.to_string(),
            emit: Arc::new(emit),
            pending: Arc::default(),
        }
    }

    fn send(&self, kind: WsEventKind) {
        let mut pending = lock(&self.pending);
        flush_locked(&self.connection_id, &self.emit, &mut pending);
        (self.emit)(WsEvent {
            connection_id: self.connection_id.clone(),
            kind,
        });
    }

    fn arm_timer(&self) {
        let (id, emit, pending) = (
            self.connection_id.clone(),
            Arc::clone(&self.emit),
            Arc::clone(&self.pending),
        );
        tokio::spawn(async move {
            tokio::time::sleep(BATCH_INTERVAL).await;
            let mut pending = lock(&pending);
            pending.timer_armed = false;
            flush_locked(&id, &emit, &mut pending);
        });
    }
}

impl WsInbound for BatchingSink {
    fn opened(&self, opened: &WsOpened) {
        self.send(WsEventKind::Opened(opened.clone()));
    }

    fn message(&self, message: &WsMessage, _raw: &[u8]) {
        let (full, arm) = {
            let mut pending = lock(&self.pending);
            pending.items.push(message.clone());
            let arm = !pending.timer_armed;
            pending.timer_armed = true;
            (pending.items.len() >= BATCH_ITEMS, arm)
        };
        if full {
            flush(&self.connection_id, &self.emit, &self.pending);
        }
        if arm {
            self.arm_timer();
        }
    }

    fn closed(&self, code: Option<u32>, reason: &str, by: WsCloseBy, error: Option<&AppError>) {
        self.send(WsEventKind::Closed {
            code,
            reason: reason.to_string(),
            by,
            error: error.cloned(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::ws::types::{WsDirection, WsFrame};

    fn message(seq: u32) -> WsMessage {
        WsMessage {
            seq,
            at_ms: 0.0,
            direction: WsDirection::Received,
            frame: WsFrame::Text,
            text: seq.to_string(),
            size: 1,
            truncated: false,
        }
    }

    fn sink() -> (BatchingSink, Arc<Mutex<Vec<WsEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let sink = BatchingSink::new("c1", move |event| captured.lock().unwrap().push(event));
        (sink, events)
    }

    fn batch_sizes(events: &Mutex<Vec<WsEvent>>) -> Vec<usize> {
        events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match &event.kind {
                WsEventKind::Messages { items } => Some(items.len()),
                _ => None,
            })
            .collect()
    }

    #[tokio::test(start_paused = true)]
    async fn messages_within_the_interval_arrive_as_one_event() {
        let (sink, events) = sink();

        (1..=5).for_each(|n| sink.message(&message(n), b""));
        assert!(batch_sizes(&events).is_empty());
        tokio::time::sleep(BATCH_INTERVAL * 2).await;

        assert_eq!(batch_sizes(&events), [5]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_full_batch_is_sent_at_once() {
        let (sink, events) = sink();

        (1..=250).for_each(|n| sink.message(&message(n), b""));
        tokio::time::sleep(BATCH_INTERVAL * 2).await;

        assert_eq!(batch_sizes(&events), [100, 100, 50]);
    }

    #[tokio::test(start_paused = true)]
    async fn opened_and_closed_flush_pending_messages_first() {
        let (sink, events) = sink();

        sink.message(&message(1), b"");
        sink.closed(Some(1000), "bye", WsCloseBy::Server, None);

        let kinds: Vec<&str> = events
            .lock()
            .unwrap()
            .iter()
            .map(|event| match event.kind {
                WsEventKind::Messages { .. } => "messages",
                WsEventKind::Closed { .. } => "closed",
                WsEventKind::Opened(_) => "opened",
            })
            .collect();
        assert_eq!(kinds, ["messages", "closed"]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_timer_batch_in_flight_is_not_overtaken_by_closed() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let sink = BatchingSink::new("c1", move |event| {
            if matches!(event.kind, WsEventKind::Messages { .. }) {
                std::thread::sleep(Duration::from_millis(300));
            }
            captured.lock().unwrap().push(event);
        });

        sink.message(&message(1), b"");
        // The timer has fired and is inside the (slow) emit of the batch.
        tokio::time::sleep(BATCH_INTERVAL + Duration::from_millis(100)).await;
        sink.closed(Some(1000), "bye", WsCloseBy::Server, None);

        let kinds: Vec<bool> = events
            .lock()
            .unwrap()
            .iter()
            .map(|event| matches!(event.kind, WsEventKind::Messages { .. }))
            .collect();
        assert_eq!(kinds, [true, false]);
    }
}
