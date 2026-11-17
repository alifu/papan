use serde_json::json;

use super::*;
use crate::error::ErrorCode;
use crate::store::request_emit::canonical_extra;
use crate::store::request_model::{parse_sections, validate_content, Extra};
use crate::store::types::Body;

fn extra(value: Value) -> Extra {
    value
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn gql(subscription: SubscriptionSettings) -> RequestContent {
    RequestContent {
        method: "POST".into(),
        url: "https://x/graphql".into(),
        kind: RequestKind::Graphql,
        body: Body::Graphql {
            query: "subscription { a }".into(),
            variables: String::new(),
            operation_name: String::new(),
        },
        subscription,
        ..RequestContent::default()
    }
}

fn assert_names(error: &crate::error::AppError, field: &str) {
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(error.message.contains(field), "{}", error.message);
}

#[test]
fn empty_section_emits_nothing() {
    assert_eq!(
        emit_subscription(&SubscriptionSettings::default(), None),
        None
    );
}

#[test]
fn defaults_are_omitted_and_values_kept() {
    let settings = SubscriptionSettings {
        url: "wss://x/ws".into(),
        init_payload: String::new(),
    };

    let emitted = emit_subscription(&settings, None).unwrap();

    assert_eq!(emitted, json!({"url": "wss://x/ws"}));
}

#[test]
fn parse_reads_both_keys() {
    let raw = extra(json!({"subscription": {"url": "wss://x", "init_payload": "{}"}}));

    let content = parse_sections("r", RequestKind::Graphql, &raw).unwrap();

    assert_eq!(content.subscription.url, "wss://x");
    assert_eq!(content.subscription.init_payload, "{}");
}

#[test]
fn unknown_keys_are_kept() {
    let raw = json!({"subscription": {"url": "wss://x", "future": 1}});

    let canonical = canonical_extra("r", RequestKind::Graphql, &extra(raw)).unwrap();

    assert_eq!(canonical["subscription"]["future"], json!(1));
}

#[test]
fn section_outside_graphql_is_refused() {
    for kind in [RequestKind::Http, RequestKind::Ws, RequestKind::Soap] {
        let raw = extra(json!({"subscription": {"url": "wss://x"}}));
        let error = parse_sections("r.http.yaml", kind, &raw).unwrap_err();
        assert_names(&error, "subscription");
        assert_names(&error, "r.http.yaml");
    }
}

#[test]
fn parse_errors_name_the_field() {
    let cases = [
        (json!({"subscription": "x"}), "subscription"),
        (json!({"subscription": {"url": 1}}), "subscription.url"),
        (
            json!({"subscription": {"init_payload": []}}),
            "subscription.init_payload",
        ),
    ];
    for (raw, field) in cases {
        let error = parse_sections("r", RequestKind::Graphql, &extra(raw)).unwrap_err();
        assert_names(&error, field);
    }
}

#[test]
fn validation_rules_name_the_field() {
    let long_url = format!("wss://{}", "a".repeat(8200));
    let big_payload = "x".repeat(MAX_INIT_PAYLOAD_BYTES + 1);
    let cases = [
        ("ftp://x/ws".to_string(), String::new(), "subscription.url"),
        (long_url, String::new(), "subscription.url"),
        (String::new(), big_payload, "subscription.init_payload"),
    ];
    for (url, init_payload, field) in cases {
        let content = gql(SubscriptionSettings { url, init_payload });
        let error = validate_content("r", &content).unwrap_err();
        assert_names(&error, field);
    }
}

#[test]
fn valid_values_pass() {
    for url in ["wss://x/ws", "ws://x", "{{base}}/ws", ""] {
        let content = gql(SubscriptionSettings {
            url: url.into(),
            init_payload: "{ \"a\": \"{{t}}\" }".into(),
        });
        assert!(validate_content("r", &content).is_ok(), "{url}");
    }
}
