//! The WebSocket engine against a local server (plan 07 §10): echo, handshake headers,
//! subprotocols, closing, pings, caps and the batching of events.

mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use papan_lib::engine::http::resolve::Resolved;
use papan_lib::engine::http::types::CookieInput;
use papan_lib::engine::http::HttpEngine;
use papan_lib::engine::ws::handshake::connect_spec;
use papan_lib::engine::ws::messages::{finish_outgoing, outgoing_content};
use papan_lib::engine::ws::sink::BatchingSink;
use papan_lib::engine::ws::types::{
    WsCloseBy, WsDirection, WsEvent, WsEventKind, WsFrame, WsMessage, WsOpened, WsOutgoing,
};
use papan_lib::engine::ws::{Outbound, WsEngine, WsInbound, MAX_CONNECTIONS};
use papan_lib::error::{AppError, ErrorCode};
use papan_lib::store::types::{
    ApiKeyPlacement, Auth, Body, KeyValue, RequestContent, RequestKind, RequestLocation, WsFormat,
    WsSettings,
};
use support::ws_server::{self, WsServer};

/// Connection ids are UUIDs (the same rule as request ids).
fn cid(n: u32) -> String {
    format!("00000000-0000-4000-8000-{n:012}")
}

const WAIT: Duration = Duration::from_secs(10);
const FUTURE_MS: f64 = 4_102_444_800_000.0;

#[derive(Debug, Clone)]
enum Seen {
    Opened,
    Message(WsMessage, Vec<u8>),
    Closed {
        code: Option<u32>,
        reason: String,
        by: WsCloseBy,
        error: Option<AppError>,
    },
}

#[derive(Default)]
struct Recorder {
    seen: Mutex<Vec<Seen>>,
}

impl WsInbound for Recorder {
    fn opened(&self, _opened: &WsOpened) {
        self.seen.lock().unwrap().push(Seen::Opened);
    }

    fn message(&self, message: &WsMessage, raw: &[u8]) {
        let seen = Seen::Message(message.clone(), raw.to_vec());
        self.seen.lock().unwrap().push(seen);
    }

    fn closed(&self, code: Option<u32>, reason: &str, by: WsCloseBy, error: Option<&AppError>) {
        self.seen.lock().unwrap().push(Seen::Closed {
            code,
            reason: reason.to_string(),
            by,
            error: error.cloned(),
        });
    }
}

impl Recorder {
    async fn until(&self, done: impl Fn(&[Seen]) -> bool) -> Vec<Seen> {
        let started = tokio::time::Instant::now();
        loop {
            let snapshot = self.seen.lock().unwrap().clone();
            if done(&snapshot) {
                return snapshot;
            }
            assert!(started.elapsed() < WAIT, "timed out; saw {snapshot:?}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    async fn messages(&self, count: usize) -> Vec<WsMessage> {
        let seen = self.until(|seen| received(seen).len() >= count).await;
        received(&seen)
    }

    async fn closed(&self) -> Seen {
        let seen = self
            .until(|seen| seen.iter().any(|s| matches!(s, Seen::Closed { .. })))
            .await;
        seen.into_iter()
            .find(|s| matches!(s, Seen::Closed { .. }))
            .unwrap()
    }
}

fn received(seen: &[Seen]) -> Vec<WsMessage> {
    seen.iter()
        .filter_map(|s| match s {
            Seen::Message(message, _) => Some(message.clone()),
            _ => None,
        })
        .collect()
}

struct Fixture {
    _dir: tempfile::TempDir,
    http: HttpEngine,
    ws: WsEngine,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let http = HttpEngine::new(&dir.path().join("cache"), dir.path(), "9.9.9").unwrap();
    Fixture {
        _dir: dir,
        http,
        ws: WsEngine::default(),
    }
}

fn scratch() -> RequestLocation {
    RequestLocation::Scratch { id: "s".into() }
}

fn ws_content(url: String) -> RequestContent {
    RequestContent {
        kind: RequestKind::Ws,
        ..RequestContent::new("GET", url)
    }
}

async fn connect(
    f: &Fixture,
    id: &str,
    content: RequestContent,
    inbound: Arc<dyn WsInbound>,
) -> Result<WsOpened, AppError> {
    let resolved = Resolved::unchanged(content);
    let spec = connect_spec(&f.http, &scratch(), &resolved, "9.9.9").await?;
    f.ws.open(id, spec, inbound).await
}

async fn open(f: &Fixture, id: &str, url: String) -> Arc<Recorder> {
    let recorder = Arc::new(Recorder::default());
    connect(f, id, ws_content(url), recorder.clone())
        .await
        .unwrap();
    recorder
}

fn outgoing(frame: WsFormat, text: &str) -> WsOutgoing {
    WsOutgoing {
        frame,
        text: text.into(),
    }
}

/// What `ws_send` does after variables are resolved, for text without variables.
async fn send(f: &Fixture, id: &str, frame: WsFormat, text: &str) -> WsMessage {
    let message = outgoing(frame, text);
    let resolved = Resolved::unchanged(outgoing_content(&message));
    let outbound = finish_outgoing(&message, &resolved).unwrap();
    f.ws.send(id, outbound).await.unwrap()
}

fn header(name: &str, value: &str) -> KeyValue {
    KeyValue {
        name: name.into(),
        value: value.into(),
        enabled: true,
    }
}

#[tokio::test]
async fn text_json_and_binary_messages_echo_and_are_logged() {
    let (f, server) = (fixture(), ws_server::spawn().await);
    let recorder = open(&f, &cid(1), server.url("/echo")).await;

    let text = send(&f, &cid(1), WsFormat::Text, "hello").await;
    let json = send(&f, &cid(1), WsFormat::Json, r#"{"a": 1}"#).await;
    let binary = send(&f, &cid(1), WsFormat::Binary, "AAEC").await;
    let echoed = recorder.messages(3).await;

    assert_eq!(
        (text.direction, text.frame),
        (WsDirection::Sent, WsFrame::Text)
    );
    assert_eq!(json.text, r#"{"a": 1}"#);
    assert_eq!(
        (binary.frame, binary.text.as_str(), binary.size),
        (WsFrame::Binary, "AAEC", 3)
    );
    assert_eq!(echoed[0].text, "hello");
    assert_eq!(echoed[1].text, r#"{"a": 1}"#);
    assert_eq!(
        (echoed[2].frame, echoed[2].text.as_str()),
        (WsFrame::Binary, "AAEC")
    );
    let log = f.ws.log(&cid(1)).unwrap();
    assert_eq!(log.len(), 6);
    assert!(log.windows(2).all(|pair| pair[0].seq < pair[1].seq));
}

#[tokio::test]
async fn headers_auth_query_and_jar_cookies_reach_the_server() {
    let (f, server) = (fixture(), ws_server::spawn().await);
    let cookie = CookieInput {
        name: "sid".into(),
        value: "abc".into(),
        domain: "127.0.0.1".into(),
        path: "/".into(),
        expires_at_ms: Some(FUTURE_MS),
        http_only: false,
        secure: false,
    };
    f.http.cookies().set(&scratch(), &cookie).await.unwrap();
    let bearer = RequestContent {
        headers: vec![header("X-Client", "papan")],
        auth: Auth::Bearer {
            token: "t0k".into(),
        },
        ..ws_content(server.url("/a?z=1"))
    };
    let keyed = RequestContent {
        auth: Auth::ApiKey {
            name: "key".into(),
            value: "k1".into(),
            placement: ApiKeyPlacement::Query,
        },
        ..ws_content(server.url("/b"))
    };

    connect(&f, &cid(1), bearer, Arc::new(Recorder::default()))
        .await
        .unwrap();
    connect(&f, &cid(2), keyed, Arc::new(Recorder::default()))
        .await
        .unwrap();

    let seen = server.handshakes();
    assert_eq!(seen[0].header("authorization"), Some("Bearer t0k"));
    assert_eq!(seen[0].header("x-client"), Some("papan"));
    assert_eq!(seen[0].header("cookie"), Some("sid=abc"));
    assert!(seen[0].header("user-agent").unwrap().starts_with("Papan/"));
    assert!(seen[1].uri.ends_with("/b?key=k1"), "{}", seen[1].uri);
}

#[tokio::test]
async fn the_negotiated_subprotocol_is_reported() {
    let (f, server) = (fixture(), ws_server::spawn_with_protocol("chat.v2").await);
    let content = RequestContent {
        ws: WsSettings {
            subprotocols: vec!["other".into(), "chat.v2".into()],
            messages: Vec::new(),
        },
        ..ws_content(server.url("/"))
    };

    let opened = connect(&f, &cid(1), content, Arc::new(Recorder::default()))
        .await
        .unwrap();

    assert_eq!(opened.protocol.as_deref(), Some("chat.v2"));
    assert_eq!(opened.status, 101);
    assert_eq!(
        server.handshakes()[0].header("sec-websocket-protocol"),
        Some("other, chat.v2")
    );
}

#[tokio::test]
async fn a_secret_in_a_message_is_masked_in_the_log_and_plain_on_the_wire() {
    let (f, server) = (fixture(), ws_server::spawn().await);
    let recorder = open(&f, &cid(1), server.url("/")).await;
    let message = outgoing(WsFormat::Text, "token={{secret}}");
    let resolved = Resolved {
        content: RequestContent {
            body: Body::Text {
                text: "token=s3cr3t-value".into(),
            },
            ..outgoing_content(&message)
        },
        secret_values: vec!["s3cr3t-value".into()],
        ..Resolved::default()
    };

    let logged =
        f.ws.send(&cid(1), finish_outgoing(&message, &resolved).unwrap())
            .await
            .unwrap();
    let echoed = recorder.until(|seen| !received(seen).is_empty()).await;

    assert!(!logged.text.contains("s3cr3t-value"), "{}", logged.text);
    let Seen::Message(_, raw) = &echoed[1] else {
        panic!("{echoed:?}");
    };
    assert_eq!(raw, b"token=s3cr3t-value");
}

#[tokio::test]
async fn closing_by_the_client_reports_the_code_and_keeps_the_log() {
    let (f, server) = (fixture(), ws_server::spawn().await);
    let first = open(&f, &cid(1), server.url("/")).await;
    let second = open(&f, &cid(2), server.url("/")).await;
    send(&f, &cid(1), WsFormat::Text, "hi").await;
    first.messages(1).await;

    f.ws.close(&cid(1), None, "").await.unwrap();
    f.ws.close(&cid(2), Some(4001), "done").await.unwrap();

    let Seen::Closed { code, by, .. } = first.closed().await else {
        panic!("not closed");
    };
    assert_eq!((code, by), (Some(1000), WsCloseBy::Client));
    let Seen::Closed {
        code, reason, by, ..
    } = second.closed().await
    else {
        panic!("not closed");
    };
    assert_eq!(
        (code, reason.as_str(), by),
        (Some(4001), "done", WsCloseBy::Client)
    );
    assert_eq!(f.ws.log(&cid(1)).unwrap().len(), 2);
    f.ws.close(&cid(1), None, "").await.unwrap();
}

#[tokio::test]
async fn a_server_close_is_shown_with_its_code_and_reason() {
    let (f, server) = (fixture(), ws_server::spawn().await);
    let recorder = open(&f, &cid(1), server.url("/")).await;

    send(&f, &cid(1), WsFormat::Text, "!close:4000:bye").await;

    let Seen::Closed {
        code,
        reason,
        by,
        error,
    } = recorder.closed().await
    else {
        panic!("not closed");
    };
    assert_eq!(
        (code, reason.as_str(), by),
        (Some(4000), "bye", WsCloseBy::Server)
    );
    assert!(error.is_none());
    let log = f.ws.log(&cid(1)).unwrap();
    assert!(log
        .iter()
        .any(|m| m.frame == WsFrame::Close && m.text == "4000 bye"));
    assert_eq!(
        f.ws.send(&cid(1), Outbound::text("x"))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Network
    );
}

#[tokio::test]
async fn a_ping_is_logged_and_server_binary_arrives_as_base64() {
    let (f, server) = (fixture(), ws_server::spawn().await);
    let recorder = open(&f, &cid(1), server.url("/")).await;

    send(&f, &cid(1), WsFormat::Text, "!ping").await;
    send(&f, &cid(1), WsFormat::Text, "!binary").await;
    let got = recorder.messages(2).await;

    assert_eq!((got[0].frame, got[0].text.as_str()), (WsFrame::Ping, "hi"));
    assert_eq!(
        (got[1].frame, got[1].text.as_str()),
        (WsFrame::Binary, "AQID")
    );
}

#[tokio::test]
async fn a_message_over_16_mib_closes_the_connection_with_an_error() {
    let (f, server) = (fixture(), ws_server::spawn().await);
    let recorder = open(&f, &cid(1), server.url("/")).await;

    send(&f, &cid(1), WsFormat::Text, "!big").await;

    let Seen::Closed { by, error, .. } = recorder.closed().await else {
        panic!("not closed");
    };
    assert_eq!(by, WsCloseBy::Error);
    assert!(error.unwrap().message.contains("16 MiB"));
}

#[tokio::test]
async fn the_1001st_message_drops_the_oldest() {
    let (f, server) = (fixture(), ws_server::spawn().await);
    let recorder = open(&f, &cid(1), server.url("/")).await;

    send(&f, &cid(1), WsFormat::Text, "!flood:1005").await;
    recorder.messages(1005).await;

    let log = f.ws.log(&cid(1)).unwrap();
    assert_eq!(log.len(), 1000);
    assert_eq!(log[0].seq, 7);
    assert_eq!(log[999].text, "m1004");
    f.ws.clear_log(&cid(1)).unwrap();
    assert!(f.ws.log(&cid(1)).unwrap().is_empty());
}

#[tokio::test]
async fn the_21st_connection_is_refused() {
    let (f, server) = (fixture(), ws_server::spawn().await);
    for n in 0..MAX_CONNECTIONS {
        open(&f, &cid(n as u32), server.url("/")).await;
    }

    let refused = connect(
        &f,
        &cid(999),
        ws_content(server.url("/")),
        Arc::new(Recorder::default()),
    )
    .await
    .unwrap_err();

    assert_eq!(refused.code, ErrorCode::InvalidInput);
    assert_eq!(f.ws.open_count(), MAX_CONNECTIONS);
    let duplicate = connect(
        &f,
        &cid(0),
        ws_content(server.url("/")),
        Arc::new(Recorder::default()),
    )
    .await
    .unwrap_err();
    assert_eq!(duplicate.code, ErrorCode::InvalidInput);
}

#[tokio::test]
async fn events_are_batched_into_fewer_events_than_messages() {
    let (f, server) = (fixture(), ws_server::spawn().await);
    let events: Arc<Mutex<Vec<WsEvent>>> = Arc::default();
    let sink_events = Arc::clone(&events);
    let sink = Arc::new(BatchingSink::new(&cid(1), move |event| {
        sink_events.lock().unwrap().push(event);
    }));
    connect(&f, &cid(1), ws_content(server.url("/")), sink)
        .await
        .unwrap();

    send(&f, &cid(1), WsFormat::Text, "!flood:300").await;
    let started = tokio::time::Instant::now();
    let batches = loop {
        let batches: Vec<usize> = events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match &event.kind {
                WsEventKind::Messages { items } => Some(items.len()),
                _ => None,
            })
            .collect();
        if batches.iter().sum::<usize>() >= 300 {
            break batches;
        }
        assert!(started.elapsed() < WAIT, "timed out: {batches:?}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    };

    assert!(batches.len() < 300, "{batches:?}");
    assert!(batches.iter().all(|size| *size <= 100));
    assert!(matches!(
        events.lock().unwrap()[0].kind,
        WsEventKind::Opened(_)
    ));
    let _: &WsServer = &server;
}
