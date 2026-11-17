use super::*;
use crate::engine::http::build::{build, Prepared, PreparedBody};
use crate::error::ErrorCode;
use crate::store::types::{KeyValue, RequestKind, SoapSettings, SoapVersion};

fn graphql(query: &str, variables: &str, operation_name: &str) -> RequestContent {
    RequestContent {
        body: Body::Graphql {
            query: query.into(),
            variables: variables.into(),
            operation_name: operation_name.into(),
        },
        kind: RequestKind::Graphql,
        ..RequestContent::new("POST", "https://x.test/graphql")
    }
}

fn soap(version: SoapVersion, action: &str) -> RequestContent {
    RequestContent {
        body: Body::Xml {
            text: "<e/>".into(),
        },
        kind: RequestKind::Soap,
        soap: SoapSettings {
            version,
            action: action.into(),
            wsdl: None,
        },
        ..RequestContent::new("POST", "https://x.test/svc")
    }
}

fn sent_body(prepared: &Prepared) -> String {
    match &prepared.body {
        PreparedBody::Bytes(bytes) => String::from_utf8(bytes.clone()).unwrap(),
        other => panic!("not bytes: {other:?}"),
    }
}

fn headers(prepared: &Prepared, name: &str) -> Vec<String> {
    prepared
        .headers
        .iter()
        .filter(|(header, _)| header.as_str().eq_ignore_ascii_case(name))
        .map(|(_, value)| value.to_str().unwrap().to_string())
        .collect()
}

#[test]
fn graphql_body_is_json_with_optional_operation_name_and_variables() {
    let bare = build(&graphql("{ a }", "  \n", ""), "1").unwrap();
    let full = build(
        &graphql("query A { a }", "{\"id\": 12345678901234567890}", "A"),
        "1",
    )
    .unwrap();

    assert_eq!(sent_body(&bare), r#"{"query":"{ a }"}"#);
    assert_eq!(
        sent_body(&full),
        r#"{"query":"query A { a }","operationName":"A","variables":{"id": 12345678901234567890}}"#
    );
    let parsed: serde_json::Value = serde_json::from_str(&sent_body(&full)).unwrap();
    assert_eq!(parsed["operationName"], "A");
    assert_eq!(headers(&full, "content-type"), ["application/json"]);
    assert_eq!(
        headers(&full, "accept"),
        ["application/graphql-response+json, application/json"]
    );
}

#[test]
fn graphql_variables_must_be_a_json_object() {
    for variables in ["[1]", "\"x\"", "{oops", "3"] {
        let error = build(&graphql("{ a }", variables, ""), "1").unwrap_err();

        assert_eq!(error.code, ErrorCode::InvalidInput, "{variables}");
        assert_eq!(error.message, "GraphQL variables must be a JSON object.");
    }
}

#[test]
fn soap_11_sends_text_xml_and_a_quoted_soap_action_even_when_empty() {
    let with_action = build(&soap(SoapVersion::V11, "urn:Add"), "1").unwrap();
    let empty = build(&soap(SoapVersion::V11, ""), "1").unwrap();

    assert_eq!(
        headers(&with_action, "content-type"),
        ["text/xml; charset=utf-8"]
    );
    assert_eq!(headers(&with_action, "soapaction"), ["\"urn:Add\""]);
    assert_eq!(headers(&empty, "soapaction"), ["\"\""]);
    assert_eq!(sent_body(&with_action), "<e/>");
}

#[test]
fn soap_12_puts_the_action_in_the_content_type_and_sends_no_soap_action() {
    let with_action = build(&soap(SoapVersion::V12, "urn:Add"), "1").unwrap();
    let empty = build(&soap(SoapVersion::V12, ""), "1").unwrap();

    assert_eq!(
        headers(&with_action, "content-type"),
        ["application/soap+xml; charset=utf-8; action=\"urn:Add\""]
    );
    assert_eq!(
        headers(&empty, "content-type"),
        ["application/soap+xml; charset=utf-8"]
    );
    assert!(headers(&with_action, "soapaction").is_empty());
}

#[test]
fn a_user_header_of_the_same_name_wins_over_derived_ones() {
    let user = |name: &str, value: &str| KeyValue {
        name: name.into(),
        value: value.into(),
        enabled: true,
    };
    let content = RequestContent {
        headers: vec![user("Content-Type", "text/xml"), user("SOAPAction", "x")],
        ..soap(SoapVersion::V11, "urn:Add")
    };
    let gql = RequestContent {
        headers: vec![user("accept", "application/json")],
        ..graphql("{ a }", "", "")
    };

    let prepared = build(&content, "1").unwrap();
    let gql_prepared = build(&gql, "1").unwrap();

    assert_eq!(headers(&prepared, "content-type"), ["text/xml"]);
    assert_eq!(headers(&prepared, "soapaction"), ["x"]);
    assert_eq!(headers(&gql_prepared, "accept"), ["application/json"]);
}

#[test]
fn an_xml_body_of_an_http_request_keeps_application_xml() {
    let content = RequestContent {
        kind: RequestKind::Http,
        soap: SoapSettings::default(),
        ..soap(SoapVersion::V11, "")
    };

    let prepared = build(&content, "1").unwrap();

    assert_eq!(headers(&prepared, "content-type"), ["application/xml"]);
    assert!(headers(&prepared, "soapaction").is_empty());
}

#[test]
fn a_quote_in_the_soap_action_is_refused_at_build_time() {
    for version in [SoapVersion::V11, SoapVersion::V12] {
        let error = soap_headers(&SoapSettings {
            version,
            action: "urn:\"x".into(),
            wsdl: None,
        })
        .unwrap_err();

        assert_eq!(error.code, ErrorCode::InvalidInput);
        assert!(error.message.contains("SOAP action"), "{}", error.message);
    }
}
