use std::sync::Mutex as StdMutex;

use serde_json::{json, Value};

use super::*;
use crate::engine::ws::types::WsDirection;

#[derive(Default)]
struct Spy {
    events: StdMutex<Vec<String>>,
}

impl WsInbound for Spy {
    fn opened(&self, _opened: &WsOpened) {
        self.events.lock().unwrap().push("opened".into());
    }
    fn message(&self, _message: &WsMessage, _raw: &[u8]) {
        self.events.lock().unwrap().push("message".into());
    }
    fn closed(&self, _code: Option<u32>, _reason: &str, _by: WsCloseBy, _e: Option<&AppError>) {
        self.events.lock().unwrap().push("closed".into());
    }
}

const ID: &str = "00000000-0000-4000-8000-000000000001";

struct Rig {
    driver: GqlWsDriver,
    spy: Arc<Spy>,
    replies: mpsc::Receiver<Reply>,
    ready: Ready,
}

fn rig(init: Option<&str>) -> Rig {
    let spy = Arc::new(Spy::default());
    let masker = Masker::new(vec!["S3CRET".into()]);
    let (driver, replies, ready) = GqlWsDriver::new(
        ID,
        spy.clone(),
        init,
        "{\"query\":\"subscription{a(t:\\\"S3CRET\\\")}\"}",
        masker,
    );
    Rig {
        driver,
        spy,
        replies,
        ready,
    }
}

fn opened(protocol: Option<&str>) -> WsOpened {
    WsOpened {
        connection_id: ID.into(),
        url: "ws://x".into(),
        protocol: protocol.map(str::to_string),
        status: 101,
        headers: Vec::new(),
    }
}

fn frame(text: &str) -> (WsMessage, Vec<u8>) {
    let message = WsMessage {
        seq: 1,
        at_ms: 0.0,
        direction: WsDirection::Received,
        frame: WsFrame::Text,
        text: text.into(),
        size: text.len() as u32,
        truncated: false,
    };
    (message, text.as_bytes().to_vec())
}

fn receive(rig: &Rig, text: &str) {
    let (message, raw) = frame(text);
    rig.driver.message(&message, &raw);
}

fn next_reply(rig: &mut Rig) -> Reply {
    rig.replies.try_recv().expect("a reply")
}

fn sent_json(reply: Reply) -> (Value, String) {
    match reply {
        Reply::Send(Outbound {
            payload: Payload::Text(text),
            shown,
        }) => (serde_json::from_str(&text).unwrap(), shown),
        other => panic!("not a text send: {other:?}"),
    }
}

fn ack(rig: &Rig) {
    receive(rig, r#"{"type":"connection_ack"}"#);
}

#[tokio::test]
async fn opening_sends_connection_init_with_the_payload() {
    let mut rig = rig(Some("{\"token\":\"S3CRET\"}"));

    rig.driver.opened(&opened(Some(SUBPROTOCOL)));

    let (init, shown) = sent_json(next_reply(&mut rig));
    assert_eq!(
        init,
        json!({"type": "connection_init", "payload": {"token": "S3CRET"}})
    );
    assert!(!shown.contains("S3CRET"), "{shown}");
}

#[tokio::test]
async fn ack_sends_subscribe_masked_in_the_log_and_resolves_ready() {
    let mut rig = rig(None);
    rig.driver.opened(&opened(Some(SUBPROTOCOL)));
    let _ = next_reply(&mut rig);

    ack(&rig);

    let (subscribe, shown) = sent_json(next_reply(&mut rig));
    assert_eq!(subscribe["type"], "subscribe");
    assert_eq!(subscribe["id"], ID);
    assert!(subscribe.to_string().contains("S3CRET"));
    assert!(!shown.contains("S3CRET"), "{shown}");
    assert!(rig.ready.await.unwrap().is_ok());
}

#[tokio::test]
async fn the_wrong_protocol_closes_with_1002_and_fails_ready() {
    let mut rig = rig(None);

    rig.driver.opened(&opened(None));

    assert!(matches!(
        next_reply(&mut rig),
        Reply::Close { code: 1002, .. }
    ));
    let error = rig.ready.await.unwrap().unwrap_err();
    assert!(
        error.message.contains("graphql-transport-ws"),
        "{}",
        error.message
    );
    assert!(error.message.contains("Legacy"), "{}", error.message);
}

#[tokio::test(start_paused = true)]
async fn no_ack_in_ten_seconds_closes_with_4504() {
    let mut rig = rig(None);
    rig.driver.opened(&opened(Some(SUBPROTOCOL)));
    let _ = next_reply(&mut rig);

    tokio::time::sleep(ACK_TIMEOUT + Duration::from_millis(1)).await;

    assert!(matches!(
        next_reply(&mut rig),
        Reply::Close { code: 4504, .. }
    ));
    let error = rig.ready.await.unwrap().unwrap_err();
    assert!(error.message.contains("10 s"));
}

#[tokio::test(start_paused = true)]
async fn an_ack_stops_the_timer() {
    let mut rig = rig(None);
    rig.driver.opened(&opened(Some(SUBPROTOCOL)));
    ack(&rig);
    let _ = (next_reply(&mut rig), next_reply(&mut rig));

    tokio::time::sleep(ACK_TIMEOUT * 2).await;

    assert!(rig.replies.try_recv().is_err());
}

#[tokio::test]
async fn a_server_ping_is_answered_with_its_payload() {
    let mut rig = rig(None);
    rig.driver.opened(&opened(Some(SUBPROTOCOL)));
    ack(&rig);
    let _ = (next_reply(&mut rig), next_reply(&mut rig));

    receive(&rig, r#"{"type":"ping","payload":{"n":1}}"#);
    receive(&rig, r#"{"type":"pong"}"#);

    let (pong, _) = sent_json(next_reply(&mut rig));
    assert_eq!(pong, json!({"type": "pong", "payload": {"n": 1}}));
    assert!(rig.replies.try_recv().is_err());
}

#[tokio::test]
async fn a_second_ack_closes_with_4400() {
    let mut rig = rig(None);
    rig.driver.opened(&opened(Some(SUBPROTOCOL)));
    ack(&rig);
    let _ = (next_reply(&mut rig), next_reply(&mut rig));

    ack(&rig);

    assert!(matches!(
        next_reply(&mut rig),
        Reply::Close { code: 4400, .. }
    ));
}

#[tokio::test]
async fn next_before_the_ack_closes_with_4400_and_fails_ready() {
    let mut rig = rig(None);
    rig.driver.opened(&opened(Some(SUBPROTOCOL)));
    let _ = next_reply(&mut rig);

    receive(
        &rig,
        &format!(r#"{{"id":"{ID}","type":"next","payload":{{}}}}"#),
    );

    assert!(matches!(
        next_reply(&mut rig),
        Reply::Close { code: 4400, .. }
    ));
    assert!(rig.ready.await.unwrap().is_err());
}

#[tokio::test]
async fn error_and_complete_end_the_operation_with_a_normal_close() {
    for ending in ["error", "complete"] {
        let mut rig = rig(None);
        rig.driver.opened(&opened(Some(SUBPROTOCOL)));
        ack(&rig);
        let _ = (next_reply(&mut rig), next_reply(&mut rig));

        receive(
            &rig,
            &format!(r#"{{"id":"{ID}","type":"{ending}","payload":[]}}"#),
        );

        assert!(
            matches!(next_reply(&mut rig), Reply::Close { code: 1000, .. }),
            "{ending}"
        );
    }
}

#[tokio::test]
async fn a_message_for_another_operation_closes_with_4400() {
    let mut rig = rig(None);
    rig.driver.opened(&opened(Some(SUBPROTOCOL)));
    ack(&rig);
    let _ = (next_reply(&mut rig), next_reply(&mut rig));

    receive(&rig, r#"{"id":"other","type":"next","payload":{}}"#);

    assert!(matches!(
        next_reply(&mut rig),
        Reply::Close { code: 4400, .. }
    ));
}

#[tokio::test]
async fn a_close_before_the_ack_fails_ready_with_the_code_message() {
    let rig = rig(None);
    rig.driver.opened(&opened(Some(SUBPROTOCOL)));

    rig.driver
        .closed(Some(4401), "nope", WsCloseBy::Server, None);

    let error = rig.ready.await.unwrap().unwrap_err();
    assert!(error.message.contains("unauthorized"), "{}", error.message);
}

#[tokio::test]
async fn an_unknown_close_code_shows_code_and_masked_reason() {
    let rig = rig(None);
    rig.driver.opened(&opened(Some(SUBPROTOCOL)));

    rig.driver
        .closed(Some(1011), "boom S3CRET", WsCloseBy::Server, None);

    let message = rig.ready.await.unwrap().unwrap_err().message;
    assert!(
        message.contains("1011") && !message.contains("S3CRET"),
        "{message}"
    );
}

#[tokio::test]
async fn everything_is_forwarded_to_the_inner_sink_in_order() {
    let rig = rig(None);

    rig.driver.opened(&opened(Some(SUBPROTOCOL)));
    ack(&rig);
    rig.driver.closed(Some(1000), "", WsCloseBy::Server, None);

    let events = rig.spy.events.lock().unwrap().clone();
    assert_eq!(events, ["opened", "message", "closed"]);
}
