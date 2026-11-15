//! GraphQL subscriptions over `graphql-transport-ws` against real and scripted servers
//! (plan 06b §9): ordering, auth, masking, stop, timeouts and the shared connection cap.

mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use papan_lib::engine::graphql::subscription_service::Subscriptions;
use papan_lib::engine::http::resolve::Resolved;
use papan_lib::engine::http::HttpEngine;
use papan_lib::engine::ws::types::{WsCloseBy, WsDirection, WsMessage, WsOpened};
use papan_lib::engine::ws::{WsEngine, WsInbound, MAX_CONNECTIONS};
use papan_lib::error::{AppError, ErrorCode};
use papan_lib::store::types::{
    Auth, Body, RequestContent, RequestKind, RequestLocation, SubscriptionSettings,
};
use support::gql_ws_server::{self, Script, TOKEN};
use support::ws_server;

const WAIT: Duration = Duration::from_secs(10);
const SECRET: &str = "init-token-5521";

fn cid(n: u32) -> String {
    format!("00000000-0000-4000-8000-{n:012}")
}

#[derive(Default)]
struct Recorder {
    messages: Mutex<Vec<WsMessage>>,
    closed: Mutex<Option<(Option<u32>, WsCloseBy)>>,
}

impl WsInbound for Recorder {
    fn opened(&self, _opened: &WsOpened) {}
    fn message(&self, message: &WsMessage, _raw: &[u8]) {
        self.messages.lock().unwrap().push(message.clone());
    }
    fn closed(&self, code: Option<u32>, _reason: &str, by: WsCloseBy, _error: Option<&AppError>) {
        *self.closed.lock().unwrap() = Some((code, by));
    }
}

impl Recorder {
    async fn until(&self, done: impl Fn(&Recorder) -> bool) {
        let started = tokio::time::Instant::now();
        while !done(self) {
            assert!(started.elapsed() < WAIT, "timed out");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn texts(&self) -> Vec<String> {
        self.messages
            .lock()
            .unwrap()
            .iter()
            .map(|m| m.text.clone())
            .collect()
    }
}

struct Rig {
    _dir: tempfile::TempDir,
    http: HttpEngine,
    ws: WsEngine,
    subs: Subscriptions,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let http = HttpEngine::new(&dir.path().join("cache"), dir.path(), "9.9.9").unwrap();
    Rig {
        _dir: dir,
        http,
        ws: WsEngine::default(),
        subs: Subscriptions::default(),
    }
}

fn draft(url: &str, query: &str, variables: &str, init_payload: &str) -> RequestContent {
    RequestContent {
        kind: RequestKind::Graphql,
        body: Body::Graphql {
            query: query.into(),
            variables: variables.into(),
            operation_name: String::new(),
        },
        subscription: SubscriptionSettings {
            url: String::new(),
            init_payload: init_payload.into(),
        },
        ..RequestContent::new("POST", url)
    }
}

fn location() -> RequestLocation {
    RequestLocation::Scratch { id: "s".into() }
}

async fn subscribe(
    rig: &Rig,
    id: &str,
    content: RequestContent,
    secrets: &[&str],
) -> (Result<WsOpened, AppError>, Arc<Recorder>) {
    let recorder = Arc::new(Recorder::default());
    let resolved = Resolved {
        template_url: content.url.clone(),
        content: content.clone(),
        secret_values: secrets.iter().map(|s| s.to_string()).collect(),
        ..Resolved::default()
    };
    let result = rig
        .subs
        .subscribe(
            &rig.ws,
            &rig.http,
            id,
            &location(),
            &content,
            &resolved,
            recorder.clone(),
            "9.9.9",
        )
        .await;
    (result, recorder)
}

const TICKS: &str = "subscription($n: Int!) { ticks(count: $n) }";

fn with_token() -> String {
    format!("{{\"token\": \"{TOKEN}\"}}")
}

#[tokio::test]
async fn results_arrive_in_order_then_complete_and_the_socket_closes() {
    let (rig, server) = (rig(), gql_ws_server::spawn_schema().await);
    let content = draft(&server.url(), TICKS, "{\"n\": 3}", &with_token());

    let (opened, recorder) = subscribe(&rig, &cid(1), content, &[]).await;

    assert_eq!(
        opened.unwrap().protocol.as_deref(),
        Some("graphql-transport-ws")
    );
    recorder.until(|r| r.closed.lock().unwrap().is_some()).await;
    let received: Vec<String> = rig
        .ws
        .log(&cid(1))
        .unwrap()
        .into_iter()
        .filter(|m| m.direction == WsDirection::Received)
        .map(|m| m.text)
        .collect();
    let types: Vec<String> = received
        .iter()
        .filter_map(|t| serde_json::from_str::<serde_json::Value>(t).ok())
        .filter_map(|v| v["type"].as_str().map(String::from))
        .collect();
    assert_eq!(
        types,
        ["connection_ack", "next", "next", "next", "complete"]
    );
    assert!(received[1].contains("\"ticks\":0") && received[3].contains("\"ticks\":2"));
    assert_eq!(
        recorder.closed.lock().unwrap().unwrap().1,
        WsCloseBy::Client
    );
}

#[tokio::test]
async fn bearer_and_init_payload_reach_the_server_and_secrets_stay_out_of_the_log() {
    let (rig, server) = (rig(), gql_ws_server::spawn_schema().await);
    let mut content = draft(&server.url(), TICKS, "{\"n\": 1}", &with_token());
    content.auth = Auth::Bearer {
        token: "bearer-secret-77".into(),
    };

    let (opened, recorder) = subscribe(&rig, &cid(1), content, &[SECRET, "bearer-secret-77"]).await;
    opened.unwrap();
    recorder.until(|r| r.closed.lock().unwrap().is_some()).await;

    assert_eq!(
        server.authorizations(),
        [Some("Bearer bearer-secret-77".to_string())]
    );
    assert_eq!(server.inits()[0]["token"], TOKEN);
    let log = rig.ws.log(&cid(1)).unwrap();
    let sent: Vec<&WsMessage> = log
        .iter()
        .filter(|m| m.direction == WsDirection::Sent)
        .collect();
    assert!(sent.len() >= 2);
    for message in &log {
        assert!(!message.text.contains(SECRET), "{}", message.text);
    }
}

#[tokio::test]
async fn a_rejected_init_is_the_command_error_without_the_token() {
    let (rig, server) = (rig(), gql_ws_server::spawn_schema().await);
    let content = draft(
        &server.url(),
        TICKS,
        "{\"n\": 1}",
        "{\"token\": \"wrong-7\"}",
    );

    let (opened, _recorder) = subscribe(&rig, &cid(1), content, &["wrong-7"]).await;

    let error = opened.unwrap_err();
    assert!(
        error.message.contains("before the subscription started"),
        "{}",
        error.message
    );
    assert!(!error.message.contains("wrong-7"));
}

#[tokio::test]
async fn close_codes_before_the_ack_become_messages() {
    let rig = rig();
    for (code, text) in [
        (4401, "unauthorized"),
        (4409, "already exists"),
        (4429, "too many"),
    ] {
        let server = gql_ws_server::spawn_script(Script::CloseOnInit(code)).await;
        let content = draft(&server.url(), TICKS, "", "");

        let (opened, _r) = subscribe(&rig, &cid(u32::from(code)), content, &[]).await;

        let message = opened.unwrap_err().message;
        assert!(message.contains(text), "{code}: {message}");
    }
}

#[tokio::test]
async fn a_server_that_never_acks_fails_after_ten_seconds_with_4504() {
    let (rig, server) = (rig(), gql_ws_server::spawn_script(Script::NoAck).await);
    let content = draft(&server.url(), TICKS, "", "");

    let (opened, recorder) = subscribe(&rig, &cid(1), content, &[]).await;

    assert!(opened.unwrap_err().message.contains("10 s"));
    recorder.until(|r| r.closed.lock().unwrap().is_some()).await;
    assert_eq!(recorder.closed.lock().unwrap().unwrap().0, Some(4504));
}

#[tokio::test]
async fn a_server_without_the_subprotocol_gets_the_legacy_error() {
    let (rig, server) = (rig(), ws_server::spawn().await);
    let content = draft(&server.url("/"), TICKS, "", "");

    let (opened, _recorder) = subscribe(&rig, &cid(1), content, &[]).await;

    let message = opened.unwrap_err().message;
    assert!(
        message.contains("graphql-transport-ws") && message.contains("Legacy"),
        "{message}"
    );
}

#[tokio::test]
async fn a_server_ping_is_answered_with_a_pong() {
    let (rig, server) = (rig(), gql_ws_server::spawn_script(Script::Ping).await);
    let (opened, recorder) =
        subscribe(&rig, &cid(1), draft(&server.url(), TICKS, "", ""), &[]).await;
    opened.unwrap();

    recorder
        .until(|r| r.texts().iter().any(|t| t.contains("\"next\"")))
        .await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert!(
        server.types().contains(&"pong".to_string()),
        "{:?}",
        server.types()
    );
}

#[tokio::test]
async fn stop_sends_complete_then_closes_with_1000() {
    let (rig, server) = (rig(), gql_ws_server::spawn_script(Script::Stream).await);
    let (opened, recorder) =
        subscribe(&rig, &cid(1), draft(&server.url(), TICKS, "", ""), &[]).await;
    opened.unwrap();
    recorder
        .until(|r| r.texts().iter().any(|t| t.contains("\"next\"")))
        .await;

    rig.subs.unsubscribe(&rig.ws, &cid(1)).await.unwrap();
    recorder.until(|r| r.closed.lock().unwrap().is_some()).await;

    let types = server.types();
    let complete = types.iter().position(|t| t == "complete").unwrap();
    let close = types.iter().position(|t| t == "<close>").unwrap();
    assert!(complete < close, "{types:?}");
    assert_eq!(recorder.closed.lock().unwrap().unwrap().0, Some(1000));
}

#[tokio::test]
async fn stopping_a_real_stream_midway_ends_the_server_side_stream() {
    let (rig, server) = (rig(), gql_ws_server::spawn_schema().await);
    let content = draft(
        &server.url(),
        "subscription { ticks(count: 1000, delayMs: 20) }",
        "",
        &with_token(),
    );
    let (opened, recorder) = subscribe(&rig, &cid(1), content, &[]).await;
    opened.unwrap();
    recorder
        .until(|r| r.texts().iter().any(|t| t.contains("\"next\"")))
        .await;

    rig.subs.unsubscribe(&rig.ws, &cid(1)).await.unwrap();
    recorder.until(|r| r.closed.lock().unwrap().is_some()).await;

    let started = tokio::time::Instant::now();
    while server.dropped_streams() == 0 {
        assert!(started.elapsed() < WAIT, "the server kept streaming");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn a_stop_that_races_the_connect_wins() {
    let (rig, addr) = (Arc::new(rig()), ws_server::spawn_hanging().await);
    let content = draft(&format!("http://{addr}/"), TICKS, "", "");
    let starting = {
        let rig = Arc::clone(&rig);
        tokio::spawn(async move { subscribe(&rig, &cid(1), content, &[]).await.0 })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;

    rig.subs.unsubscribe(&rig.ws, &cid(1)).await.unwrap();

    let error = starting.await.unwrap().unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(rig.ws.open_count(), 0);
}

#[tokio::test]
async fn the_twenty_first_connection_is_refused() {
    let (rig, server) = (rig(), gql_ws_server::spawn_script(Script::Stream).await);
    for n in 1..=MAX_CONNECTIONS as u32 {
        let (opened, _r) = subscribe(&rig, &cid(n), draft(&server.url(), TICKS, "", ""), &[]).await;
        opened.unwrap();
    }

    let (extra, _r) = subscribe(&rig, &cid(99), draft(&server.url(), TICKS, "", ""), &[]).await;

    assert_eq!(extra.unwrap_err().code, ErrorCode::InvalidInput);
}

#[tokio::test]
async fn unsubscribing_an_unknown_or_closed_subscription_is_not_an_error() {
    let rig = rig();

    rig.subs.unsubscribe(&rig.ws, &cid(5)).await.unwrap();
}
