use std::sync::Mutex;

use prost_reflect::{DescriptorPool, MethodDescriptor};
use tokio::sync::mpsc;

use super::*;
use crate::engine::grpc::codec::{from_json, to_json};
use crate::engine::grpc::protos::tests::pool_of;
use crate::engine::ws::log::WsLog;

const PROTO: &str = r#"syntax = "proto3";
package t;
message M { string text = 1; }
service Svc {
  rpc Unary (M) returns (M);
  rpc Server (M) returns (stream M);
  rpc Client (stream M) returns (M);
  rpc Bidi (stream M) returns (stream M);
}
"#;

pub(crate) fn pool() -> DescriptorPool {
    pool_of(&[("t.proto", PROTO)], "t.proto")
}

pub(crate) fn method(name: &str) -> MethodDescriptor {
    let service = pool().get_service_by_name("t.Svc").unwrap();
    let found = service.methods().find(|method| method.name() == name);
    found.unwrap()
}

pub(crate) fn msg(text: &str) -> DynamicMessage {
    let descriptor = pool().get_message_by_name("t.M").unwrap();
    from_json(&descriptor, &format!(r#"{{"text":"{text}"}}"#)).unwrap()
}

/// The server side of a [`FakeTransport`].
pub(crate) struct FakeServer {
    /// Response steps; dropping it ends the stream with no status.
    pub steps: mpsc::UnboundedSender<Result<DynamicMessage, Status>>,
    /// Releases `open` (the response headers).
    pub headers: Option<oneshot::Sender<Result<MetadataMap, Status>>>,
    /// The requests received as JSON, then `<end>` at the half-close.
    pub received: Arc<Mutex<Vec<String>>>,
}

impl FakeServer {
    pub(crate) fn respond(&mut self) {
        let headers = self.headers.take().unwrap();
        let _ = headers.send(Ok(MetadataMap::new()));
    }

    pub(crate) fn ok(&self) {
        self.steps.send(Err(Status::ok(""))).unwrap();
    }

    pub(crate) fn received(&self) -> Vec<String> {
        self.received.lock().unwrap().clone()
    }

    /// Whether the client dropped the response stream (a cancel on the wire).
    pub(crate) fn client_gone(&self) -> bool {
        self.steps.is_closed()
    }
}

pub(crate) struct FakeTransport {
    steps: mpsc::UnboundedReceiver<Result<DynamicMessage, Status>>,
    headers: oneshot::Receiver<Result<MetadataMap, Status>>,
    received: Arc<Mutex<Vec<String>>>,
}

pub(crate) fn fake() -> (Box<FakeTransport>, FakeServer) {
    let (steps_tx, steps) = mpsc::unbounded_channel();
    let (headers_tx, headers) = oneshot::channel();
    let received = Arc::new(Mutex::new(Vec::new()));
    let transport = FakeTransport {
        steps,
        headers,
        received: Arc::clone(&received),
    };
    let server = FakeServer {
        steps: steps_tx,
        headers: Some(headers_tx),
        received,
    };
    (Box::new(transport), server)
}

impl Transport for FakeTransport {
    fn open(self: Box<Self>, mut requests: Requests) -> BoxFuture<'static, Result<Opened, Status>> {
        let received = self.received;
        tokio::spawn(async move {
            while let Some(request) = requests.next().await {
                let text = to_json(&request).unwrap();
                received.lock().unwrap().push(text);
            }
            received.lock().unwrap().push("<end>".into());
        });
        let (headers, steps) = (self.headers, self.steps);
        Box::pin(async move {
            let headers = headers
                .await
                .unwrap_or_else(|_| Err(Status::unavailable("gone")))?;
            let responses = futures_util::stream::unfold(steps, |mut steps| async {
                steps.recv().await.map(|step| (step, steps))
            });
            Ok(Opened {
                headers,
                responses: responses.boxed(),
            })
        })
    }
}

/// Records everything a call reports, in order.
#[derive(Default)]
pub(crate) struct Recorder {
    pub events: Mutex<Vec<String>>,
    pub finished: Mutex<Vec<Finished>>,
}

impl Recorder {
    pub(crate) fn labelled(label: &str, events: &Arc<Recorder>) -> Arc<SharedRecorder> {
        Arc::new(SharedRecorder {
            label: label.to_string(),
            target: Arc::clone(events),
        })
    }

    pub(crate) fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }

    fn push(&self, text: String) {
        self.events.lock().unwrap().push(text);
    }
}

impl GrpcInbound for Recorder {
    fn headers(&self, metadata: Vec<crate::store::types::KeyValue>) {
        self.push(format!("headers:{}", metadata.len()));
    }

    fn message(&self, message: &WsMessage) {
        let direction = match message.direction {
            WsDirection::Sent => "sent",
            WsDirection::Received => "received",
        };
        self.push(format!("{direction}:{}", message.text));
    }

    fn finished(&self, finished: Finished) {
        self.push(format!("finished:{}", finished.status.name));
        self.finished.lock().unwrap().push(finished);
    }
}

/// A view on a shared [`Recorder`] that prefixes every event (one per attempt).
pub(crate) struct SharedRecorder {
    label: String,
    target: Arc<Recorder>,
}

impl GrpcInbound for SharedRecorder {
    fn headers(&self, metadata: Vec<crate::store::types::KeyValue>) {
        self.target
            .push(format!("{}headers:{}", self.label, metadata.len()));
    }

    fn message(&self, message: &WsMessage) {
        self.target.push(format!("{}{}", self.label, message.text));
    }

    fn finished(&self, finished: Finished) {
        self.target
            .push(format!("{}finished:{}", self.label, finished.status.name));
        self.target.finished.lock().unwrap().push(finished);
    }
}

struct Running {
    server: FakeServer,
    recorder: Arc<Recorder>,
    log: SharedLog,
    cancel: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

fn spawn(deadline: Option<Duration>, secrets: &[&str]) -> Running {
    let (transport, server) = fake();
    let recorder = Arc::new(Recorder::default());
    let log: SharedLog = Arc::new(Mutex::new(WsLog::default()));
    let (cancel, cancel_rx) = oneshot::channel();
    let first = msg("hi");
    let task = CallTask {
        id: "c1".into(),
        attempt: 1,
        transport,
        requests: futures_util::stream::once(async move { first }).boxed(),
        deadline,
        masker: Masker::new(secrets.iter().map(|s| s.to_string()).collect()),
        log: Arc::clone(&log),
        inbound: Arc::clone(&recorder) as Arc<dyn GrpcInbound>,
        cancel: cancel_rx,
    };
    Running {
        server,
        recorder,
        log,
        cancel: Some(cancel),
        task: tokio::spawn(run(task)),
    }
}

fn only_finished(recorder: &Recorder) -> Finished {
    let finished = recorder.finished.lock().unwrap();
    assert_eq!(finished.len(), 1, "finished exactly once");
    finished[0].clone()
}

#[tokio::test]
async fn headers_messages_then_the_status_with_trailers_last() {
    let mut call = spawn(None, &["s3cret"]);
    let mut trailers = MetadataMap::new();
    trailers.insert("x-trailer-a", "1".parse().unwrap());
    trailers.insert("x-auth-token", "abc".parse().unwrap());

    call.server.respond();
    call.server.steps.send(Ok(msg("one"))).unwrap();
    call.server.steps.send(Ok(msg("has s3cret"))).unwrap();
    let ok = Status::with_metadata(Code::Ok, "", trailers);
    call.server.steps.send(Err(ok)).unwrap();
    call.task.await.unwrap();

    let events = call.recorder.events();
    assert_eq!(events[0], "headers:0");
    assert_eq!(events[1], r#"received:{"text":"one"}"#);
    assert!(!events[2].contains("s3cret"), "{events:?}");
    assert_eq!(events[3], "finished:OK");
    assert_eq!(events.len(), 4);
    let finished = only_finished(&call.recorder);
    assert_eq!((finished.status.code, finished.by), (0, GrpcEndBy::Server));
    let trailer = |name: &str| {
        let found = finished.trailers.iter().find(|row| row.name == name);
        found.unwrap().value.clone()
    };
    assert_eq!(trailer("x-trailer-a"), "1");
    assert_eq!(trailer("x-auth-token"), "***");
    let logged = lock_log(&call.log).snapshot();
    assert_eq!(logged.len(), 2);
    assert_eq!(logged[0].direction, WsDirection::Received);
    assert_eq!(call.server.received(), [r#"{"text":"hi"}"#, "<end>"]);
}

#[tokio::test]
async fn a_cancel_drops_the_stream_and_finishes_as_cancelled_by_the_client() {
    let mut call = spawn(None, &[]);
    call.server.respond();
    call.server.steps.send(Ok(msg("one"))).unwrap();
    while call.recorder.events().len() < 2 {
        tokio::task::yield_now().await;
    }

    call.cancel.take().unwrap().send(()).unwrap();
    call.task.await.unwrap();

    let finished = only_finished(&call.recorder);
    assert_eq!(finished.status.name, "CANCELLED");
    assert_eq!(finished.by, GrpcEndBy::Client);
    assert!(call.server.client_gone());
    assert_eq!(call.recorder.events().last().unwrap(), "finished:CANCELLED");
}

#[tokio::test]
async fn a_cancel_before_the_headers_and_a_dropped_handle_both_cancel() {
    let mut waiting = spawn(None, &[]);
    waiting.cancel.take().unwrap().send(()).unwrap();
    waiting.task.await.unwrap();
    assert_eq!(waiting.recorder.events(), ["finished:CANCELLED"]);

    let mut dropped = spawn(None, &[]);
    dropped.server.respond();
    drop(dropped.cancel.take());
    dropped.task.await.unwrap();
    assert_eq!(only_finished(&dropped.recorder).status.name, "CANCELLED");
}

#[tokio::test(start_paused = true)]
async fn the_client_timer_ends_the_call_with_deadline_exceeded() {
    let mut call = spawn(Some(Duration::from_millis(200)), &[]);
    call.server.respond();

    call.task.await.unwrap();

    let finished = only_finished(&call.recorder);
    assert_eq!(finished.status.code, Code::DeadlineExceeded as u32);
    assert_eq!(finished.status.name, "DEADLINE_EXCEEDED");
    assert_eq!(finished.by, GrpcEndBy::Client);
    assert!(finished.elapsed_ms >= 200.0);
    assert!(call.cancel.is_some());
}

#[tokio::test]
async fn a_status_instead_of_headers_is_reported_with_details_and_masked() {
    let mut call = spawn(None, &["hunter2"]);
    let status = Status::with_details(
        Code::Unauthenticated,
        "bad token hunter2",
        vec![1_u8, 2, 3].into(),
    );

    call.server
        .headers
        .take()
        .unwrap()
        .send(Err(status))
        .unwrap();
    call.task.await.unwrap();

    assert_eq!(call.recorder.events(), ["finished:UNAUTHENTICATED"]);
    let finished = only_finished(&call.recorder);
    assert_eq!(finished.status.code, 16);
    assert!(!finished.status.message.contains("hunter2"));
    assert_eq!(finished.status.details_base64.as_deref(), Some("AQID"));
    assert_eq!(finished.by, GrpcEndBy::Server);
    assert_eq!(finished.error, None);
}

#[tokio::test]
async fn a_broken_connection_ends_by_error_and_a_silent_end_is_unknown() {
    let mut broken = spawn(None, &["hunter2"]);
    broken.server.respond();
    let source = std::io::Error::other("reset near hunter2");
    let status = Status::from_error(Box::new(source));
    broken.server.steps.send(Err(status)).unwrap();
    broken.task.await.unwrap();

    let finished = only_finished(&broken.recorder);
    assert_eq!(finished.by, GrpcEndBy::Error);
    let error = finished.error.unwrap();
    assert_eq!(error.code, ErrorCode::Network);
    assert!(!error.detail.unwrap().contains("hunter2"));

    let mut silent = spawn(None, &[]);
    silent.server.respond();
    let FakeServer { steps, .. } = silent.server;
    drop(steps);
    silent.task.await.unwrap();
    assert_eq!(only_finished(&silent.recorder).status.name, "UNKNOWN");
}

#[test]
fn every_code_has_its_canonical_name() {
    let names: Vec<&str> = (0..=16).map(|code| code_name(Code::from(code))).collect();

    assert_eq!(names[0], "OK");
    assert_eq!(names[4], "DEADLINE_EXCEEDED");
    assert_eq!(names[8], "RESOURCE_EXHAUSTED");
    assert_eq!(names[14], "UNAVAILABLE");
    assert_eq!(names[16], "UNAUTHENTICATED");
    let distinct: std::collections::HashSet<&&str> = names.iter().collect();
    assert_eq!(distinct.len(), 17);
}
