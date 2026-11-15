//! Where a call reports to (plan 08 §5). The call task knows only [`GrpcInbound`];
//! [`GrpcBatchingSink`] turns it into [`GrpcEvent`]s: messages batched (50 ms or 100
//! items) and flushed before every other event. Every emit happens under the sink's
//! lock, so a timer batch in flight can never be overtaken (the M7 fix).

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super::types::{GrpcEndBy, GrpcEvent, GrpcEventKind, GrpcStatus};
use crate::engine::ws::types::WsMessage;
use crate::error::AppError;
use crate::store::types::KeyValue;

pub const BATCH_INTERVAL: Duration = Duration::from_millis(50);
pub const BATCH_ITEMS: usize = 100;

/// How a call ended. Everything is already masked.
#[derive(Debug, Clone, PartialEq)]
pub struct Finished {
    pub status: GrpcStatus,
    pub trailers: Vec<KeyValue>,
    pub by: GrpcEndBy,
    pub elapsed_ms: f64,
    pub error: Option<AppError>,
}

/// Receives what happens on one call, in order. Called from the call task: keep it quick.
pub trait GrpcInbound: Send + Sync {
    /// The response headers (masked). Not called when the call fails before them.
    fn headers(&self, metadata: Vec<KeyValue>);
    /// A message as logged, sent or received.
    fn message(&self, message: &WsMessage);
    /// Exactly once, and last.
    fn finished(&self, finished: Finished);
}

type Emit = Arc<dyn Fn(GrpcEvent) + Send + Sync>;

#[derive(Default)]
struct Pending {
    items: Vec<WsMessage>,
    timer_armed: bool,
}

#[derive(Clone)]
struct Target {
    call_id: String,
    attempt: u32,
    emit: Emit,
}

impl Target {
    fn send(&self, kind: GrpcEventKind) {
        (self.emit)(GrpcEvent {
            call_id: self.call_id.clone(),
            attempt: self.attempt,
            kind,
        });
    }

    /// Emits the pending messages. The caller holds the lock for the whole emit.
    fn flush(&self, pending: &mut Pending) {
        let items = std::mem::take(&mut pending.items);
        if !items.is_empty() {
            self.send(GrpcEventKind::Messages { items });
        }
    }
}

pub struct GrpcBatchingSink {
    target: Target,
    pending: Arc<Mutex<Pending>>,
}

fn lock(pending: &Mutex<Pending>) -> MutexGuard<'_, Pending> {
    pending.lock().unwrap_or_else(PoisonError::into_inner)
}

impl GrpcBatchingSink {
    pub fn new(
        call_id: &str,
        attempt: u32,
        emit: impl Fn(GrpcEvent) + Send + Sync + 'static,
    ) -> Self {
        Self {
            target: Target {
                call_id: call_id.to_string(),
                attempt,
                emit: Arc::new(emit),
            },
            pending: Arc::default(),
        }
    }

    fn send(&self, kind: GrpcEventKind) {
        let mut pending = lock(&self.pending);
        self.target.flush(&mut pending);
        self.target.send(kind);
    }

    fn arm_timer(&self) {
        let (target, pending) = (self.target.clone(), Arc::clone(&self.pending));
        tokio::spawn(async move {
            tokio::time::sleep(BATCH_INTERVAL).await;
            let mut pending = lock(&pending);
            pending.timer_armed = false;
            target.flush(&mut pending);
        });
    }
}

impl GrpcInbound for GrpcBatchingSink {
    fn headers(&self, metadata: Vec<KeyValue>) {
        self.send(GrpcEventKind::Headers { metadata });
    }

    fn message(&self, message: &WsMessage) {
        let arm = {
            let mut pending = lock(&self.pending);
            pending.items.push(message.clone());
            if pending.items.len() >= BATCH_ITEMS {
                self.target.flush(&mut pending);
            }
            let arm = !pending.timer_armed;
            pending.timer_armed = true;
            arm
        };
        if arm {
            self.arm_timer();
        }
    }

    fn finished(&self, finished: Finished) {
        self.send(GrpcEventKind::Finished {
            status: finished.status,
            trailers: finished.trailers,
            by: finished.by,
            elapsed_ms: finished.elapsed_ms,
            error: finished.error,
        });
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::engine::ws::types::{WsDirection, WsFrame};

    pub(crate) fn message(seq: u32) -> WsMessage {
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

    pub(crate) fn ok_finished() -> Finished {
        Finished {
            status: GrpcStatus {
                code: 0,
                name: "OK".into(),
                message: String::new(),
                details_base64: None,
            },
            trailers: Vec::new(),
            by: GrpcEndBy::Server,
            elapsed_ms: 1.0,
            error: None,
        }
    }

    fn sink() -> (GrpcBatchingSink, Arc<Mutex<Vec<GrpcEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let sink = GrpcBatchingSink::new("c1", 3, move |event| {
            captured.lock().unwrap().push(event);
        });
        (sink, events)
    }

    fn kinds(events: &Mutex<Vec<GrpcEvent>>) -> Vec<String> {
        let name = |event: &GrpcEvent| match &event.kind {
            GrpcEventKind::Headers { .. } => "headers".to_string(),
            GrpcEventKind::Messages { items } => format!("messages:{}", items.len()),
            GrpcEventKind::Finished { .. } => "finished".to_string(),
        };
        events.lock().unwrap().iter().map(name).collect()
    }

    #[tokio::test(start_paused = true)]
    async fn messages_within_the_interval_arrive_as_one_event_with_id_and_attempt() {
        let (sink, events) = sink();

        (1..=5).for_each(|n| sink.message(&message(n)));
        assert!(kinds(&events).is_empty());
        tokio::time::sleep(BATCH_INTERVAL * 2).await;

        assert_eq!(kinds(&events), ["messages:5"]);
        let first = events.lock().unwrap()[0].clone();
        assert_eq!((first.call_id.as_str(), first.attempt), ("c1", 3));
    }

    #[tokio::test(start_paused = true)]
    async fn a_full_batch_is_sent_at_once_and_the_timer_is_rearmed() {
        let (sink, events) = sink();

        (1..=250).for_each(|n| sink.message(&message(n)));
        tokio::time::sleep(BATCH_INTERVAL * 2).await;
        sink.message(&message(251));
        tokio::time::sleep(BATCH_INTERVAL * 2).await;

        assert_eq!(
            kinds(&events),
            ["messages:100", "messages:100", "messages:50", "messages:1"]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn headers_and_finished_flush_pending_messages_first() {
        let (sink, events) = sink();

        sink.message(&message(1));
        sink.headers(Vec::new());
        sink.message(&message(2));
        sink.message(&message(3));
        sink.finished(ok_finished());
        tokio::time::sleep(BATCH_INTERVAL * 2).await;

        assert_eq!(
            kinds(&events),
            ["messages:1", "headers", "messages:2", "finished"]
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_timer_batch_in_flight_is_not_overtaken_by_finished() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let sink = GrpcBatchingSink::new("c1", 1, move |event| {
            if matches!(event.kind, GrpcEventKind::Messages { .. }) {
                std::thread::sleep(Duration::from_millis(300));
            }
            captured.lock().unwrap().push(event);
        });

        sink.message(&message(1));
        // The timer has fired and is inside the (slow) emit of the batch.
        tokio::time::sleep(BATCH_INTERVAL + Duration::from_millis(100)).await;
        sink.finished(ok_finished());

        assert_eq!(kinds(&events), ["messages:1", "finished"]);
    }
}
