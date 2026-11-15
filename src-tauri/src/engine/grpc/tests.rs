use std::sync::Arc;

use tokio::sync::oneshot;

use super::call::tests::{fake, method, msg, FakeServer, Recorder};
use super::*;

fn prepared(name: &str) -> (Prepared, FakeServer) {
    let (transport, server) = fake();
    let method = method(name);
    let one_request = !takes_stream(schema::method_kind(&method));
    let prepared = Prepared {
        transport,
        method,
        first: one_request.then(|| msg("first")),
        deadline: None,
        masker: Masker::new(vec!["s3cret".into()]),
        url: "http://server:1/".into(),
    };
    (prepared, server)
}

async fn start(
    engine: &GrpcEngine,
    id: &str,
    name: &str,
    recorder: &Arc<Recorder>,
) -> (GrpcStarted, FakeServer) {
    let (prepared, server) = prepared(name);
    let sink = |attempt: u32| -> Arc<dyn GrpcInbound> {
        Recorder::labelled(&format!("{attempt}:"), recorder)
    };
    let started = engine.start(id, async { Ok(prepared) }, sink).await;
    (started.unwrap(), server)
}

/// A call id as the webview makes them (a UUID).
fn id(n: usize) -> String {
    format!("00000000-0000-4000-8000-{n:012}")
}

async fn until(condition: impl Fn() -> bool) {
    for _ in 0..2000 {
        if condition() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    panic!("the condition was never met");
}

fn no_secrets() -> Masker {
    Masker::default()
}

#[tokio::test]
async fn a_unary_call_sends_its_message_and_keeps_the_log_after_it_finished() {
    let engine = GrpcEngine::default();
    let recorder = Arc::new(Recorder::default());

    let (started, mut server) = start(&engine, &id(1), "Unary", &recorder).await;
    assert_eq!(started.call_id, id(1));
    assert_eq!(started.attempt, 1);
    assert_eq!(started.path, "/t.Svc/Unary");
    assert_eq!(started.kind, GrpcMethodKind::Unary);
    assert_eq!(engine.live_count(), 1);
    server.respond();
    server.steps.send(Ok(msg("reply"))).unwrap();
    server.ok();
    until(|| engine.live_count() == 0).await;

    assert_eq!(
        recorder.events(),
        [
            r#"1:{"text":"first"}"#,
            "1:headers:0",
            r#"1:{"text":"reply"}"#,
            "1:finished:OK"
        ]
    );
    assert_eq!(server.received(), [r#"{"text":"first"}"#, "<end>"]);
    let log = engine.log(&id(1));
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].direction, WsDirection::Sent);
    let late = engine.send(&id(1), "{}", &no_secrets()).await.unwrap_err();
    assert_eq!(late.message, "The call has ended.");
    assert_eq!(
        engine.end(&id(1)).unwrap_err().message,
        "The call has ended."
    );
    engine.cancel(&id(1));
    engine.clear_log(&id(1));
    assert!(engine.log(&id(1)).is_empty());
}

#[tokio::test]
async fn a_client_stream_sends_on_demand_and_end_half_closes() {
    let engine = GrpcEngine::default();
    let recorder = Arc::new(Recorder::default());
    let (started, mut server) = start(&engine, &id(1), "Client", &recorder).await;
    assert_eq!(started.kind, GrpcMethodKind::ClientStreaming);
    assert!(engine.log(&id(1)).is_empty());

    let secrets = Masker::new(vec!["hunter2".into()]);
    let text = r#"{"text":"a hunter2 s3cret"}"#;
    let one = engine.send(&id(1), text, &secrets).await.unwrap();
    engine
        .send(&id(1), r#"{"text":"b"}"#, &no_secrets())
        .await
        .unwrap();
    let invalid = engine.send(&id(1), r#"{"hunter2":1}"#, &secrets).await;
    engine.end(&id(1)).unwrap();
    engine.end(&id(1)).unwrap();
    let after_end = engine.send(&id(1), "{}", &no_secrets()).await.unwrap_err();
    until(|| server.received().len() == 3).await;

    assert_eq!(one.direction, WsDirection::Sent);
    assert!(!one.text.contains("hunter2") && !one.text.contains("s3cret"));
    assert!(!invalid.unwrap_err().message.contains("hunter2"));
    assert!(after_end.message.contains("request stream has ended"));
    assert_eq!(
        server.received(),
        [r#"{"text":"a hunter2 s3cret"}"#, r#"{"text":"b"}"#, "<end>"]
    );
    server.respond();
    server.ok();
    until(|| engine.live_count() == 0).await;
    assert_eq!(engine.log(&id(1)).len(), 2);
}

#[tokio::test]
async fn a_unary_method_takes_no_more_messages() {
    let engine = GrpcEngine::default();
    let recorder = Arc::new(Recorder::default());
    let (_, _server) = start(&engine, &id(1), "Server", &recorder).await;

    let error = engine.send(&id(1), "{}", &no_secrets()).await.unwrap_err();

    assert!(error.message.contains("takes one message"));
    engine.end(&id(1)).unwrap();
}

#[tokio::test]
async fn a_restart_cancels_the_old_call_and_never_interleaves_attempts() {
    let engine = GrpcEngine::default();
    let recorder = Arc::new(Recorder::default());
    let (_, mut old) = start(&engine, &id(1), "Bidi", &recorder).await;
    old.respond();
    old.steps.send(Ok(msg("old"))).unwrap();
    until(|| recorder.events().len() == 2).await;

    let (started, mut new) = start(&engine, &id(1), "Bidi", &recorder).await;
    new.respond();
    new.ok();
    until(|| engine.live_count() == 0).await;

    assert_eq!(started.attempt, 2);
    assert!(old.client_gone());
    assert_eq!(
        recorder.events(),
        [
            "1:headers:0",
            r#"1:{"text":"old"}"#,
            "1:finished:CANCELLED",
            "2:headers:0",
            "2:finished:OK"
        ]
    );
    assert_eq!(engine.live_count(), 0);
    assert!(engine.log(&id(1)).is_empty());
}

#[tokio::test]
async fn a_cancel_during_prepare_wins_and_the_start_reports_cancelled() {
    let engine = GrpcEngine::default();
    let recorder = Arc::new(Recorder::default());
    let (entered_tx, entered) = oneshot::channel();
    let prepare = async {
        let _ = entered_tx.send(());
        std::future::pending::<Result<Prepared, AppError>>().await
    };
    let events = Arc::clone(&recorder);
    let sink = move |_| -> Arc<dyn GrpcInbound> { events };
    let starting = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.start(&id(1), prepare, sink).await })
    };
    entered.await.unwrap();
    assert_eq!(engine.live_count(), 1);

    engine.cancel(&id(1));
    let error = starting.await.unwrap().unwrap_err();

    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(engine.live_count(), 0);
    assert!(recorder.events().is_empty());
}

#[tokio::test]
async fn a_cancel_after_prepare_finished_still_wins() {
    let engine = GrpcEngine::default();
    let (cancel, _kept) = oneshot::channel();
    let (attempt, _) = lock(&engine.registry).reserve(&id(1), cancel).unwrap();
    engine.cancel(&id(1));
    let (prepared, _server) = prepared("Unary");
    let recorder = Arc::new(Recorder::default());
    let sink = |_| -> Arc<dyn GrpcInbound> { Arc::clone(&recorder) as _ };

    let error = engine.launch(&id(1), attempt, prepared, sink).unwrap_err();

    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(engine.live_count(), 0);
    assert!(recorder.events().is_empty());
}

#[tokio::test]
async fn a_failed_start_keeps_the_previous_log_and_frees_the_slot() {
    let engine = GrpcEngine::default();
    let recorder = Arc::new(Recorder::default());
    let (_, mut server) = start(&engine, &id(1), "Unary", &recorder).await;
    server.respond();
    server.ok();
    until(|| engine.live_count() == 0).await;
    let sink = |_| -> Arc<dyn GrpcInbound> { Arc::clone(&recorder) as _ };

    let failing = async { Err(AppError::invalid_input("The service `x` is not defined.")) };
    let error = engine.start(&id(1), failing, sink).await.unwrap_err();

    assert!(error.message.contains("not defined"));
    assert_eq!(engine.live_count(), 0);
    assert_eq!(engine.log(&id(1)).len(), 1);
    let (started, _server) = start(&engine, &id(1), "Unary", &recorder).await;
    assert_eq!(started.attempt, 3);
}

#[tokio::test]
async fn at_most_twenty_calls_run_and_a_restart_does_not_count_twice() {
    let engine = GrpcEngine::default();
    let recorder = Arc::new(Recorder::default());
    let mut servers = Vec::new();
    for n in 0..MAX_CALLS {
        servers.push(start(&engine, &id(n), "Bidi", &recorder).await.1);
    }
    let sink = |_| -> Arc<dyn GrpcInbound> { Arc::clone(&recorder) as _ };

    let (extra, _server) = prepared("Bidi");
    let refused = engine.start(&id(99), async { Ok(extra) }, sink).await;
    let (restarted, _server) = start(&engine, &id(0), "Bidi", &recorder).await;

    assert!(refused.unwrap_err().message.contains("At most 20"));
    assert_eq!(restarted.attempt, 2);
    assert_eq!(engine.live_count(), MAX_CALLS);
}

#[tokio::test]
async fn unknown_and_invalid_ids_are_handled_without_a_crash() {
    let engine = GrpcEngine::default();
    let recorder = Arc::new(Recorder::default());
    let sink = |_| -> Arc<dyn GrpcInbound> { Arc::clone(&recorder) as _ };
    let (prepared, _server) = prepared("Unary");

    let invalid = engine.start("../x", async { Ok(prepared) }, sink).await;

    assert_eq!(invalid.unwrap_err().message, "That call id is not valid.");
    assert!(engine.log(&id(7)).is_empty());
    engine.cancel(&id(7));
    engine.clear_log(&id(7));
    assert!(engine.end(&id(7)).is_err());
    assert!(engine.send(&id(7), "{}", &no_secrets()).await.is_err());
}

#[tokio::test]
async fn finished_logs_are_kept_up_to_the_cap() {
    let engine = GrpcEngine::default();
    let recorder = Arc::new(Recorder::default());
    for n in 0..=MAX_CLOSED_LOGS {
        let (_, mut server) = start(&engine, &id(n), "Unary", &recorder).await;
        server.respond();
        server.ok();
        until(|| engine.live_count() == 0).await;
    }

    assert_eq!(lock(&engine.registry).len(), MAX_CLOSED_LOGS);
    assert!(engine.log(&id(0)).is_empty());
    assert_eq!(engine.log(&id(1)).len(), 1);
}

#[tokio::test]
async fn attempt_numbers_keep_growing_after_the_entry_was_evicted() {
    let engine = GrpcEngine::default();
    let recorder = Arc::new(Recorder::default());
    for n in 0..=MAX_CLOSED_LOGS {
        let (_, mut server) = start(&engine, &id(n), "Unary", &recorder).await;
        server.respond();
        server.ok();
        until(|| engine.live_count() == 0).await;
    }
    assert!(engine.log(&id(0)).is_empty(), "id 0 was evicted");

    let (again, _server) = start(&engine, &id(0), "Unary", &recorder).await;

    assert_eq!(again.attempt, 2);
}

struct PanicTransport;

impl Transport for PanicTransport {
    fn open(
        self: Box<Self>,
        _requests: call::Requests,
    ) -> futures_util::future::BoxFuture<'static, Result<call::Opened, tonic::Status>> {
        panic!("the transport broke");
    }
}

#[tokio::test]
async fn a_panic_in_the_call_still_frees_the_slot() {
    let engine = GrpcEngine::default();
    let recorder = Arc::new(Recorder::default());
    let (mut prepared, _server) = prepared("Unary");
    prepared.transport = Box::new(PanicTransport);
    let sink = |_: u32| -> Arc<dyn GrpcInbound> { Arc::clone(&recorder) as Arc<dyn GrpcInbound> };

    engine
        .start(&id(1), async { Ok(prepared) }, sink)
        .await
        .unwrap();
    until(|| engine.live_count() == 0).await;
    let (again, _server) = start(&engine, &id(1), "Unary", &recorder).await;

    assert_eq!(again.attempt, 2);
}

/// Reads its requests only after `go`, and never answers.
struct GatedTransport {
    go: oneshot::Receiver<()>,
    seen: Arc<Mutex<usize>>,
}

impl Transport for GatedTransport {
    fn open(
        self: Box<Self>,
        mut requests: call::Requests,
    ) -> futures_util::future::BoxFuture<'static, Result<call::Opened, tonic::Status>> {
        Box::pin(async move {
            let _ = self.go.await;
            while requests.next().await.is_some() {
                *self.seen.lock().unwrap() += 1;
            }
            std::future::pending().await
        })
    }
}

#[tokio::test]
async fn a_send_waiting_for_room_does_not_go_out_after_end() {
    let engine = GrpcEngine::default();
    let recorder = Arc::new(Recorder::default());
    let (go_tx, go) = oneshot::channel();
    let seen = Arc::new(Mutex::new(0));
    let (mut prepared, _server) = prepared("Client");
    prepared.transport = Box::new(GatedTransport {
        go,
        seen: Arc::clone(&seen),
    });
    let sink = |_: u32| -> Arc<dyn GrpcInbound> { Arc::clone(&recorder) as Arc<dyn GrpcInbound> };
    engine
        .start(&id(1), async { Ok(prepared) }, sink)
        .await
        .unwrap();
    for _ in 0..QUEUE_DEPTH {
        engine.send(&id(1), "{}", &no_secrets()).await.unwrap();
    }
    let waiting = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.send(&id(1), "{}", &no_secrets()).await })
    };
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    engine.end(&id(1)).unwrap();
    let _ = go_tx.send(());
    let late = waiting.await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert!(late.is_err(), "{late:?}");
    assert_eq!(*seen.lock().unwrap(), QUEUE_DEPTH);
    engine.cancel(&id(1));
}
