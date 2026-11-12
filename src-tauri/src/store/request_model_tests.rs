use serde_json::json;

use super::*;
use crate::error::ErrorCode;
use crate::store::request_emit::{canonical_extra, merge};

fn extra(value: Value) -> Extra {
    value
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn header(name: &str, value: &str) -> KeyValue {
    KeyValue {
        name: name.into(),
        value: value.into(),
        enabled: true,
    }
}

fn error_of(content: &RequestContent) -> AppError {
    validate_content("r", content).unwrap_err()
}

#[test]
fn absent_sections_parse_to_defaults() {
    let content = parse_sections("r", &Extra::new()).unwrap();

    assert_eq!(content, RequestContent::default());
}

#[test]
fn unknown_auth_and_body_types_parse_to_unsupported() {
    let raw = extra(json!({"auth": {"type": "oauth2"}, "body": {"type": "graphql"}}));

    let content = parse_sections("r", &raw).unwrap();

    assert_eq!(content.auth, Auth::Unsupported);
    assert_eq!(content.body, Body::Unsupported);
}

#[test]
fn shape_errors_name_the_dotted_field() {
    let cases = [
        (
            json!({"headers": [{"name": 1, "value": []}]}),
            "headers.value",
        ),
        (
            json!({"headers": [{"name": "a", "enabled": "no"}]}),
            "headers.enabled",
        ),
        (json!({"auth": "x"}), "auth"),
        (json!({"auth": {"name": "x"}}), "auth.type"),
        (
            json!({"settings": {"timeout_ms": 1.5}}),
            "settings.timeout_ms",
        ),
        (
            json!({"body": {"type": "form", "fields": [{"value": "v"}]}}),
            "body.fields.name",
        ),
        (
            json!({"body": {"type": "multipart", "parts": "x"}}),
            "body.parts",
        ),
    ];
    for (raw, field) in cases {
        let error = parse_sections("r", &extra(raw)).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput);
        assert!(
            error.message.contains(field),
            "{} lacks {field}",
            error.message
        );
    }
}

#[test]
fn limits_are_enforced_on_typed_content() {
    let base = RequestContent::new("GET", "https://x");
    let cases = [
        (
            RequestContent {
                headers: vec![header("a", "b"); MAX_ENTRIES + 1],
                ..base.clone()
            },
            "headers",
        ),
        (
            RequestContent {
                headers: vec![header(&"n".repeat(MAX_ENTRY_NAME_CHARS + 1), "")],
                ..base.clone()
            },
            "headers.name",
        ),
        (
            RequestContent {
                disabled_params: vec![Pair {
                    name: "a".into(),
                    value: "v".repeat(MAX_ENTRY_VALUE_CHARS + 1),
                }],
                ..base.clone()
            },
            "disabled_params.value",
        ),
        (
            RequestContent {
                body: Body::Json {
                    text: "x".repeat(MAX_BODY_TEXT_BYTES + 1),
                },
                ..base.clone()
            },
            "body.text",
        ),
        (
            RequestContent {
                body: Body::Multipart {
                    parts: vec![
                        Part {
                            name: "p".into(),
                            value: String::new(),
                            file: None,
                            enabled: true,
                        };
                        MAX_PARTS + 1
                    ],
                },
                ..base.clone()
            },
            "body.parts",
        ),
        (
            RequestContent {
                body: Body::Binary {
                    file: Some("a\nb".into()),
                },
                ..base.clone()
            },
            "body.file",
        ),
        (
            RequestContent {
                auth: Auth::Bearer {
                    token: "a\r\nX: y".into(),
                },
                ..base.clone()
            },
            "auth.token",
        ),
        (
            RequestContent {
                settings: Settings {
                    timeout_ms: MAX_TIMEOUT_MS + 1,
                    ..Settings::default()
                },
                ..base.clone()
            },
            "settings.timeout_ms",
        ),
        (RequestContent::new("get", ""), "method"),
    ];
    for (content, field) in cases {
        let error = error_of(&content);
        assert!(
            error.message.contains(field),
            "{} lacks {field}",
            error.message
        );
    }
    assert!(validate_content("r", &base).is_ok());
}

#[test]
fn canonical_form_omits_defaults_and_drops_unnamed_entries() {
    let raw = extra(json!({
        "headers": [{"name": "", "value": "x"}, {"name": "A", "value": "", "enabled": true}],
        "settings": {"follow_redirects": true, "timeout_ms": 30000, "verify_tls": true},
        "auth": {"type": "basic", "username": "", "password": ""},
        "body": {"type": "binary", "file": ""},
        "disabled_params": [],
        "other": 1,
    }));

    let canonical = canonical_extra("r", &raw).unwrap();

    assert_eq!(
        canonical,
        extra(json!({
            "headers": [{"name": "A"}],
            "auth": {"type": "basic"},
            "body": {"type": "binary"},
            "other": 1,
        }))
    );
}

#[test]
fn merge_keeps_unchanged_sections_and_rewrites_changed_ones() {
    let raw = extra(json!({
        "headers": [{"name": "A", "value": "1", "trace": true}],
        "settings": {"timeout_ms": 5000, "retries": 3},
        "auth": {"type": "oauth2", "flows": {}},
    }));
    let disk = parse_sections("r", &raw).unwrap();
    let edited = RequestContent {
        headers: vec![header("B", "2")],
        ..disk.clone()
    };

    let unchanged = merge("r", &raw, &disk).unwrap();
    let changed = merge("r", &raw, &edited).unwrap();

    assert_eq!(unchanged, raw);
    assert_eq!(changed["headers"], json!([{"name": "B", "value": "2"}]));
    assert_eq!(changed["settings"], raw["settings"]);
    assert_eq!(changed["auth"], raw["auth"]);
}

#[test]
fn merge_refuses_to_write_an_unsupported_section_over_a_known_one() {
    let raw = extra(json!({"auth": {"type": "bearer", "token": "t"}}));
    let incoming = RequestContent {
        auth: Auth::Unsupported,
        ..RequestContent::default()
    };

    let error = merge("r", &raw, &incoming).unwrap_err();

    assert!(error.message.contains("auth"), "{}", error.message);
}

#[test]
fn every_body_and_auth_type_survives_emit_and_parse() {
    let bodies = [
        Body::Json { text: "{}".into() },
        Body::Xml {
            text: "<a/>".into(),
        },
        Body::Text {
            text: String::new(),
        },
        Body::Form {
            fields: vec![KeyValue {
                enabled: false,
                ..header("a", "b")
            }],
        },
        Body::Multipart {
            parts: vec![Part {
                name: "f".into(),
                value: String::new(),
                file: Some("/tmp/x".into()),
                enabled: false,
            }],
        },
        Body::Binary {
            file: Some("/tmp/x".into()),
        },
    ];
    let auths = [
        Auth::Basic {
            username: "u".into(),
            password: "p".into(),
        },
        Auth::Bearer { token: "t".into() },
        Auth::ApiKey {
            name: "k".into(),
            value: "v".into(),
            placement: ApiKeyPlacement::Query,
        },
    ];
    let contents = bodies
        .into_iter()
        .map(|body| RequestContent {
            body,
            ..RequestContent::default()
        })
        .chain(auths.into_iter().map(|auth| RequestContent {
            auth,
            ..RequestContent::default()
        }));
    for content in contents {
        let written = merge("r", &Extra::new(), &content).unwrap();
        assert_eq!(parse_sections("r", &written).unwrap(), content);
    }
}

#[test]
fn normalise_matches_what_a_write_reads_back() {
    let content = RequestContent {
        headers: vec![header("", "x"), header("A", "1")],
        disabled_params: vec![Pair {
            name: String::new(),
            value: "v".into(),
        }],
        body: Body::Multipart {
            parts: vec![
                Part {
                    name: "p".into(),
                    value: String::new(),
                    file: Some(String::new()),
                    enabled: true,
                },
                Part {
                    name: String::new(),
                    value: "dropped".into(),
                    file: None,
                    enabled: true,
                },
            ],
        },
        ..RequestContent::new("GET", "x")
    };

    let written = merge("r", &Extra::new(), &content).unwrap();
    let read_back = RequestContent {
        method: "GET".into(),
        url: "x".into(),
        ..parse_sections("r", &written).unwrap()
    };

    assert_eq!(crate::store::request_emit::normalise(&content), read_back);
}
