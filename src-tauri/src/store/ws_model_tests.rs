use serde_json::json;

use super::*;
use crate::error::ErrorCode;
use crate::store::model::RequestFile;
use crate::store::request_emit::canonical_extra;
use crate::store::request_model::{parse_sections, validate_content, Extra};
use crate::store::types::{Scripts, SoapSettings};

fn extra(value: Value) -> Extra {
    value
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn template(name: &str, format: WsFormat, text: &str) -> WsTemplate {
    WsTemplate {
        name: name.into(),
        format,
        text: text.into(),
    }
}

fn ws_content(ws: WsSettings) -> RequestContent {
    RequestContent {
        method: "GET".into(),
        url: "wss://x/chat".into(),
        kind: RequestKind::Ws,
        ws,
        ..RequestContent::default()
    }
}

fn assert_names(error: &crate::error::AppError, field: &str) {
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(error.message.contains(field), "{}", error.message);
}

const FULL: &str = "kind: ws
method: GET
name: Chat room
url: wss://x/chat
ws:
  messages:
    - format: json
      name: Join
      text: |
        { \"type\": \"join\" }
    - format: binary
      name: Blob
      text: AAEC
    - name: Hello
      text: hi
  subprotocols:
    - chat.v2
    - graphql-transport-ws
";

#[test]
fn kind_ws_parses() {
    assert_eq!(
        crate::store::protocol_model::parse_kind("r", "ws").unwrap(),
        RequestKind::Ws
    );
}

#[test]
fn full_file_round_trips_byte_identical_and_defaults_are_omitted() {
    let file = RequestFile::parse("r.http.yaml", FULL.as_bytes()).unwrap();
    let content = file.content().unwrap();

    assert_eq!(content.ws.subprotocols, ["chat.v2", "graphql-transport-ws"]);
    assert_eq!(content.ws.messages.len(), 3);
    assert_eq!(content.ws.messages[2].format, WsFormat::Text);
    assert_eq!(file.to_yaml().unwrap(), FULL);
}

#[test]
fn an_empty_ws_section_emits_nothing() {
    let emitted = emit_ws(&WsSettings::default(), None);

    assert_eq!(emitted, None);
}

#[test]
fn unknown_keys_are_kept() {
    let raw = json!({"ws": {"subprotocols": ["a"], "future": 1}});
    let canonical = canonical_extra("r", RequestKind::Ws, &extra(raw)).unwrap();

    assert_eq!(canonical["ws"]["future"], json!(1));
}

#[test]
fn ws_section_outside_a_ws_request_is_refused() {
    let raw = extra(json!({"ws": {"subprotocols": ["a"]}}));
    let error = parse_sections("r.http.yaml", RequestKind::Http, &raw).unwrap_err();

    assert_names(&error, "ws");
    assert_names(&error, "r.http.yaml");
}

#[test]
fn parse_errors_name_the_field() {
    let cases = [
        (json!({"ws": "x"}), "ws"),
        (json!({"ws": {"subprotocols": "x"}}), "ws.subprotocols"),
        (json!({"ws": {"subprotocols": [1]}}), "ws.subprotocols"),
        (
            json!({"ws": {"messages": [{"text": "a"}]}}),
            "ws.messages.name",
        ),
        (
            json!({"ws": {"messages": [{"name": "a", "format": "xml"}]}}),
            "ws.messages.format",
        ),
    ];
    for (raw, field) in cases {
        let error = parse_sections("r", RequestKind::Ws, &extra(raw)).unwrap_err();
        assert_names(&error, field);
    }
}

#[test]
fn ws_requests_refuse_body_scripts_and_soap() {
    let body = extra(json!({"body": {"type": "text", "text": "x"}}));
    assert_names(
        &parse_sections("r", RequestKind::Ws, &body).unwrap_err(),
        "body.type",
    );
    let scripts = extra(json!({"scripts": {"pre_request": "1"}}));
    assert_names(
        &parse_sections("r", RequestKind::Ws, &scripts).unwrap_err(),
        "scripts",
    );
    let soap = RequestContent {
        soap: SoapSettings {
            action: "a".into(),
            ..SoapSettings::default()
        },
        ..ws_content(WsSettings::default())
    };
    assert_names(&validate_content("r", &soap).unwrap_err(), "soap");
    let scripted = RequestContent {
        scripts: Scripts {
            pre_request: "1".into(),
            post_response: String::new(),
        },
        ..ws_content(WsSettings::default())
    };
    assert_names(&validate_content("r", &scripted).unwrap_err(), "scripts");
}

#[test]
fn method_must_be_get_and_url_scheme_ws() {
    let post = RequestContent {
        method: "POST".into(),
        ..ws_content(WsSettings::default())
    };
    assert_names(&validate_content("r", &post).unwrap_err(), "method");

    for url in ["https://x", "ftp://x", "x.test"] {
        let bad = RequestContent {
            url: url.into(),
            ..ws_content(WsSettings::default())
        };
        assert_names(&validate_content("r", &bad).unwrap_err(), "url");
    }
    for url in ["", "ws://x", "wss://x/a", "{{base}}/chat", "WSS://x"] {
        let good = RequestContent {
            url: url.into(),
            ..ws_content(WsSettings::default())
        };
        validate_content("r", &good).unwrap();
    }
}

#[test]
fn subprotocol_limits() {
    let many: Vec<String> = (0..=MAX_SUBPROTOCOLS).map(|n| format!("p{n}")).collect();
    let cases = [
        many,
        vec!["has space".into()],
        vec![String::new()],
        vec!["a".repeat(MAX_SUBPROTOCOL_CHARS + 1)],
        vec!["a".into(), "a".into()],
        vec!["a,b".into()],
    ];
    for subprotocols in cases {
        let content = ws_content(WsSettings {
            subprotocols,
            messages: Vec::new(),
        });
        assert_names(
            &validate_content("r", &content).unwrap_err(),
            "ws.subprotocols",
        );
    }
}

#[test]
fn template_limits() {
    let many: Vec<WsTemplate> = (0..=MAX_TEMPLATES)
        .map(|n| template(&format!("t{n}"), WsFormat::Text, ""))
        .collect();
    let cases = [
        (many, "ws.messages"),
        (vec![template("", WsFormat::Text, "")], "ws.messages.name"),
        (
            vec![template(&"n".repeat(257), WsFormat::Text, "")],
            "ws.messages.name",
        ),
        (
            vec![
                template("a", WsFormat::Text, ""),
                template("a", WsFormat::Json, ""),
            ],
            "ws.messages.name",
        ),
        (
            vec![template(
                "a",
                WsFormat::Text,
                &"x".repeat(MAX_TEMPLATE_BYTES + 1),
            )],
            "ws.messages.text",
        ),
        (
            vec![template("a", WsFormat::Binary, "not base64!")],
            "ws.messages.text",
        ),
    ];
    for (messages, field) in cases {
        let content = ws_content(WsSettings {
            subprotocols: Vec::new(),
            messages,
        });
        assert_names(&validate_content("r", &content).unwrap_err(), field);
    }
}

#[test]
fn binary_template_with_variables_is_allowed_and_multiline_text_is_kept() {
    let content = ws_content(WsSettings {
        subprotocols: Vec::new(),
        messages: vec![
            template("a", WsFormat::Binary, "{{payload}}"),
            template("b", WsFormat::Text, "line1\nline2\n"),
        ],
    });

    validate_content("r", &content).unwrap();
}

#[test]
fn merge_keeps_an_unchanged_ws_section_and_replaces_a_changed_one() {
    let file = RequestFile::parse("r.http.yaml", FULL.as_bytes()).unwrap();
    let mut content = file.content().unwrap();
    content.ws.subprotocols.push("extra".into());

    let updated = file.with_content(&content).unwrap();

    assert_eq!(updated.content().unwrap().ws.subprotocols.len(), 3);
}
