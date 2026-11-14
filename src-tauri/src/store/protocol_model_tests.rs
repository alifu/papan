use serde_json::json;

use super::*;
use crate::error::ErrorCode;
use crate::store::model::RequestFile;
use crate::store::request_emit::{canonical_extra, merge};
use crate::store::request_model::{parse_sections, validate_content, Extra};

fn extra(value: Value) -> Extra {
    value
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn graphql(query: &str, variables: &str, operation_name: &str) -> Body {
    Body::Graphql {
        query: query.into(),
        variables: variables.into(),
        operation_name: operation_name.into(),
    }
}

fn graphql_content(body: Body) -> RequestContent {
    RequestContent {
        method: "POST".into(),
        body,
        kind: RequestKind::Graphql,
        ..RequestContent::default()
    }
}

fn soap_content(soap: SoapSettings) -> RequestContent {
    RequestContent {
        method: "POST".into(),
        body: Body::Xml {
            text: "<a/>".into(),
        },
        kind: RequestKind::Soap,
        soap,
        ..RequestContent::default()
    }
}

#[test]
fn kinds_parse_and_newer_kinds_are_refused_naming_the_field() {
    assert_eq!(parse_kind("r", "http").unwrap(), RequestKind::Http);
    assert_eq!(parse_kind("r", "graphql").unwrap(), RequestKind::Graphql);
    assert_eq!(parse_kind("r", "soap").unwrap(), RequestKind::Soap);
    let error = parse_kind("r.http.yaml", "grpc").unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(error.message.contains("r.http.yaml"), "{}", error.message);
    assert!(error.message.contains("kind"), "{}", error.message);
}

#[test]
fn graphql_body_parses_and_a_missing_body_is_an_empty_one() {
    let raw = extra(json!({"body": {
        "type": "graphql", "query": "{ a }", "variables": "{}", "operation_name": "A"
    }}));

    let full = parse_sections("r", RequestKind::Graphql, &raw).unwrap();
    let empty = parse_sections("r", RequestKind::Graphql, &Extra::new()).unwrap();

    assert_eq!(full.body, graphql("{ a }", "{}", "A"));
    assert_eq!(full.kind, RequestKind::Graphql);
    assert_eq!(empty.body, graphql("", "", ""));
}

#[test]
fn graphql_emit_omits_defaults_keeps_unknown_keys_and_drops_an_empty_body() {
    let raw = extra(json!({"body": {
        "type": "graphql", "query": "{ a }", "variables": "", "operation_name": "", "x": 1
    }}));
    let empty = extra(json!({"body": {"type": "graphql", "query": ""}}));

    let canonical = canonical_extra("r", RequestKind::Graphql, &raw).unwrap();
    let dropped = canonical_extra("r", RequestKind::Graphql, &empty).unwrap();

    assert_eq!(
        canonical["body"],
        json!({"type": "graphql", "query": "{ a }", "x": 1})
    );
    assert!(!dropped.contains_key(BODY));
}

#[test]
fn soap_section_parses_emits_defaults_omitted_and_keeps_unknown_keys() {
    let raw = extra(json!({"body": {"type": "xml", "text": "<a/>"}, "soap": {
        "action": "urn:Add", "version": "1.2", "zz": true,
        "wsdl": {"url": "http://x/?WSDL", "service": "S", "port": "P", "operation": "Add", "q": 2}
    }}));

    let content = parse_sections("r", RequestKind::Soap, &raw).unwrap();
    let canonical = canonical_extra("r", RequestKind::Soap, &raw).unwrap();

    assert_eq!(content.soap.version, SoapVersion::V12);
    assert_eq!(content.soap.action, "urn:Add");
    assert_eq!(content.soap.wsdl.as_ref().unwrap().operation, "Add");
    assert_eq!(canonical["soap"], raw["soap"]);
    let defaults = extra(json!({"soap": {"version": "1.1", "action": ""}}));
    let gone = canonical_extra("r", RequestKind::Soap, &defaults).unwrap();
    assert!(!gone.contains_key(SOAP));
}

#[test]
fn soap_version_accepts_a_yaml_number_and_refuses_others() {
    let number = extra(json!({"soap": {"version": 1.2}}));
    let bad = extra(json!({"soap": {"version": "2.0"}}));

    let parsed = parse_sections("r", RequestKind::Soap, &number).unwrap();
    let error = parse_sections("r", RequestKind::Soap, &bad).unwrap_err();

    assert_eq!(parsed.soap.version, SoapVersion::V12);
    assert!(error.message.contains("soap.version"), "{}", error.message);
}

#[test]
fn consistency_errors_name_the_field() {
    let cases = [
        (
            RequestKind::Http,
            json!({"body": {"type": "graphql", "query": "{a}"}}),
            "body.type",
        ),
        (
            RequestKind::Graphql,
            json!({"body": {"type": "json", "text": "{}"}}),
            "body.type",
        ),
        (
            RequestKind::Graphql,
            json!({"soap": {"action": "a"}}),
            "soap",
        ),
        (RequestKind::Http, json!({"soap": {}}), "soap"),
        (
            RequestKind::Soap,
            json!({"body": {"type": "json", "text": "{}"}}),
            "body.type",
        ),
        (
            RequestKind::Graphql,
            json!({"body": {"type": "graphql", "operation_name": "1bad"}}),
            "body.operation_name",
        ),
        (
            RequestKind::Soap,
            json!({"soap": {"wsdl": {"file": "../etc/x.wsdl"}}}),
            "soap.wsdl.file",
        ),
        (
            RequestKind::Soap,
            json!({"soap": {"action": "a\nb"}}),
            "soap.action",
        ),
        (RequestKind::Soap, json!({"soap": []}), "soap"),
        (
            RequestKind::Soap,
            json!({"soap": {"action": "urn:\"x"}}),
            "soap.action",
        ),
    ];
    for (kind, raw, field) in cases {
        let error = parse_sections("r.http.yaml", kind, &extra(raw.clone())).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{raw}");
        assert!(error.message.contains("r.http.yaml"), "{}", error.message);
        assert!(error.message.contains(field), "{raw}: {}", error.message);
    }
}

#[test]
fn graphql_must_be_post_and_limits_apply() {
    let get = RequestContent {
        method: "GET".into(),
        ..graphql_content(graphql("{a}", "", ""))
    };
    let big = graphql_content(graphql(&"a".repeat(MAX_GRAPHQL_TEXT_BYTES + 1), "", ""));
    let long_name = graphql_content(graphql("{a}", "", &"a".repeat(257)));
    let in_http = RequestContent {
        kind: RequestKind::Http,
        ..graphql_content(graphql("{a}", "", ""))
    };

    for (content, field) in [
        (get, "method"),
        (big, "body.query"),
        (long_name, "body.operation_name"),
        (in_http, "body.type"),
    ] {
        let error = validate_content("r", &content).unwrap_err();
        assert!(error.message.contains(field), "{}", error.message);
    }
    assert!(validate_content("r", &graphql_content(graphql("{a}", "{}", "_A1"))).is_ok());
}

#[test]
fn soap_settings_outside_a_soap_request_are_refused() {
    let content = RequestContent {
        soap: SoapSettings {
            action: "urn:x".into(),
            ..SoapSettings::default()
        },
        ..RequestContent::new("POST", "")
    };
    let long_action = soap_content(SoapSettings {
        action: "a".repeat(2049),
        ..SoapSettings::default()
    });

    let error = validate_content("r", &content).unwrap_err();
    let too_long = validate_content("r", &long_action).unwrap_err();

    assert!(error.message.contains("soap"), "{}", error.message);
    assert!(
        too_long.message.contains("soap.action"),
        "{}",
        too_long.message
    );
}

#[test]
fn merge_rewrites_only_a_changed_soap_section() {
    let raw =
        extra(json!({"body": {"type": "xml", "text": "<a/>"}, "soap": {"action": "a", "k": 1}}));
    let disk = parse_sections("r", RequestKind::Soap, &raw).unwrap();
    let edited = RequestContent {
        soap: SoapSettings {
            version: SoapVersion::V12,
            ..disk.soap.clone()
        },
        ..disk.clone()
    };

    assert_eq!(merge("r", RequestKind::Soap, &raw, &disk).unwrap(), raw);
    let changed = merge("r", RequestKind::Soap, &raw, &edited).unwrap();
    assert_eq!(changed["soap"], json!({"action": "a", "version": "1.2"}));
}

#[test]
fn starter_files_hold_the_default_content_of_each_kind() {
    let http = new_request_file("A".into(), RequestKind::Http).unwrap();
    let gql = new_request_file("G".into(), RequestKind::Graphql).unwrap();
    let soap = new_request_file("S".into(), RequestKind::Soap).unwrap();

    assert_eq!(
        http.to_yaml().unwrap(),
        "kind: http\nmethod: GET\nname: A\n"
    );
    assert_eq!(
        gql.to_yaml().unwrap(),
        "kind: graphql\nmethod: POST\nname: G\n"
    );
    assert_eq!(gql.content().unwrap().body, graphql("", "", ""));
    let soap_content = soap.content().unwrap();
    assert_eq!(soap_content.kind, RequestKind::Soap);
    assert_eq!(
        soap_content.body,
        Body::Xml {
            text: String::new()
        }
    );
    let back = RequestFile::parse("s", soap.to_yaml().unwrap().as_bytes()).unwrap();
    assert_eq!(back, soap);
}

#[test]
fn the_kind_of_a_file_cannot_change() {
    let file = new_request_file("A".into(), RequestKind::Http).unwrap();

    let error = file
        .with_content(&graphql_content(graphql("{a}", "", "")))
        .unwrap_err();

    assert!(error.message.contains("kind"), "{}", error.message);
}
