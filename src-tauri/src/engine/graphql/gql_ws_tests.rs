use serde_json::json;

use super::*;
use crate::error::ErrorCode;

fn as_json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[test]
fn connection_init_carries_the_payload_as_typed() {
    let with = connection_init(Some("{ \"token\": \"a\" }"));
    let without = connection_init(None);

    assert_eq!(
        as_json(&with),
        json!({"type": "connection_init", "payload": {"token": "a"}})
    );
    assert!(with.contains("{ \"token\": \"a\" }"));
    assert_eq!(as_json(&without), json!({"type": "connection_init"}));
}

#[test]
fn init_payload_check_accepts_empty_and_objects_only() {
    assert_eq!(check_init_payload("  ").unwrap(), None);
    assert_eq!(
        check_init_payload(" {\"a\":1} ").unwrap(),
        Some("{\"a\":1}")
    );
    for bad in ["[1]", "\"x\"", "{oops"] {
        assert_eq!(
            check_init_payload(bad).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }
}

#[test]
fn subscribe_wraps_the_body_and_quotes_the_id() {
    let text = subscribe("a\"b", "{\"query\":\"subscription{a}\"}");

    assert_eq!(
        as_json(&text),
        json!({"id": "a\"b", "type": "subscribe", "payload": {"query": "subscription{a}"}})
    );
}

#[test]
fn pong_echoes_the_ping_payload() {
    assert_eq!(as_json(&pong(None)), json!({"type": "pong"}));
    assert_eq!(
        as_json(&pong(Some(&json!({"n": 1})))),
        json!({"type": "pong", "payload": {"n": 1}})
    );
}

#[test]
fn complete_names_the_operation() {
    assert_eq!(
        as_json(&complete("x")),
        json!({"id": "x", "type": "complete"})
    );
}

#[test]
fn parse_table() {
    let cases = [
        (r#"{"type":"connection_ack"}"#, ServerMessage::ConnectionAck),
        (r#"{"type":"ping"}"#, ServerMessage::Ping(None)),
        (
            r#"{"type":"ping","payload":{"a":1}}"#,
            ServerMessage::Ping(Some(json!({"a": 1}))),
        ),
        (r#"{"type":"pong"}"#, ServerMessage::Pong),
        (
            r#"{"id":"1","type":"next","payload":{"data":{"a":1}}}"#,
            ServerMessage::Next {
                id: "1".into(),
                payload: json!({"data": {"a": 1}}),
            },
        ),
        (
            r#"{"id":"1","type":"error","payload":[{"message":"x"}]}"#,
            ServerMessage::Error {
                id: "1".into(),
                payload: json!([{"message": "x"}]),
            },
        ),
        (
            r#"{"id":"1","type":"complete"}"#,
            ServerMessage::Complete { id: "1".into() },
        ),
    ];
    for (text, expected) in cases {
        assert_eq!(parse(text, "1").unwrap(), expected, "{text}");
    }
}

#[test]
fn parse_rejects_bad_json_unknown_types_and_foreign_ids() {
    let cases = [
        "not json",
        r#"{"payload":{}}"#,
        r#"{"type":"subscribe"}"#,
        r#"{"id":"2","type":"next","payload":{}}"#,
        r#"{"type":"complete"}"#,
        r#"{"id":"2","type":"error","payload":[]}"#,
    ];
    for text in cases {
        let error = parse(text, "1").unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{text}");
    }
}

#[test]
fn close_messages_cover_the_protocol_codes() {
    for code in [4400, 4401, 4403, 4406, 4408, 4409, 4429, 4500, 4504] {
        assert!(close_message(code).is_some(), "{code}");
    }
    assert_eq!(close_message(1000), None);
    assert!(close_message(4401).unwrap().contains("unauthorized"));
}
