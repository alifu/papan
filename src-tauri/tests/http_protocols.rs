//! GraphQL and SOAP sends and `fetch_document` against the local test server (plan 06 §5).

mod support;

use std::sync::Arc;

use papan_lib::engine::http::service::{FetchLimits, HopCheck};
use papan_lib::engine::http::HttpEngine;
use papan_lib::error::{AppError, ErrorCode};
use papan_lib::store::types::{
    Auth, Body, RequestContent, RequestKind, RequestLocation, SoapSettings, SoapVersion,
};
use serde_json::Value;
use support::server::{self, TestServer};
use url::Url;

struct Fixture {
    _dir: tempfile::TempDir,
    engine: HttpEngine,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let engine = HttpEngine::new(&dir.path().join("cache"), dir.path(), "9.9.9").unwrap();
    Fixture { _dir: dir, engine }
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn scratch() -> RequestLocation {
    RequestLocation::Scratch { id: "s".into() }
}

async fn echo(f: &Fixture, content: &RequestContent) -> Value {
    let id = new_id();
    f.engine.send(&id, &scratch(), content).await.unwrap();
    serde_json::from_str(&f.engine.read_body(&id).unwrap().data).unwrap()
}

fn echoed_header(echo: &Value, name: &str) -> Option<String> {
    echo["headers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str().unwrap().to_string())
}

fn echoed_text(echo: &Value) -> String {
    use base64::Engine as _;
    let encoded = echo["body"].as_str().unwrap();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap();
    String::from_utf8(bytes).unwrap()
}

fn soap(server: &TestServer, version: SoapVersion, action: &str) -> RequestContent {
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
        ..RequestContent::new("POST", server.url("/echo"))
    }
}

#[tokio::test]
async fn soap_11_and_12_send_their_content_type_and_action() {
    let server = server::spawn().await;
    let f = fixture();

    let v11 = echo(&f, &soap(&server, SoapVersion::V11, "urn:Add")).await;
    let v12 = echo(&f, &soap(&server, SoapVersion::V12, "urn:Add")).await;

    assert_eq!(
        echoed_header(&v11, "content-type").as_deref(),
        Some("text/xml; charset=utf-8")
    );
    assert_eq!(
        echoed_header(&v11, "soapaction").as_deref(),
        Some("\"urn:Add\"")
    );
    assert_eq!(
        echoed_header(&v12, "content-type").as_deref(),
        Some("application/soap+xml; charset=utf-8; action=\"urn:Add\"")
    );
    assert_eq!(echoed_header(&v12, "soapaction"), None);
    assert_eq!(echoed_text(&v11), "<e/>");
}

#[tokio::test]
async fn graphql_is_posted_as_json_and_bad_variables_send_nothing() {
    let server = server::spawn().await;
    let f = fixture();
    let graphql = |variables: &str| RequestContent {
        body: Body::Graphql {
            query: "{ a }".into(),
            variables: variables.into(),
            operation_name: String::new(),
        },
        kind: RequestKind::Graphql,
        ..RequestContent::new("POST", server.url("/echo"))
    };

    let sent = echo(&f, &graphql("{\"x\": 1}")).await;
    let hits = server.hits();
    let refused = f
        .engine
        .send(&new_id(), &scratch(), &graphql("[1]"))
        .await
        .unwrap_err();

    assert_eq!(
        echoed_text(&sent),
        r#"{"query":"{ a }","variables":{"x": 1}}"#
    );
    assert_eq!(
        echoed_header(&sent, "content-type").as_deref(),
        Some("application/json")
    );
    assert_eq!(refused.message, "GraphQL variables must be a JSON object.");
    assert_eq!(server.hits(), hits);
}

async fn fetch(
    f: &Fixture,
    url: &str,
    auth: &Auth,
    limits: FetchLimits,
) -> Result<papan_lib::engine::http::service::FetchedDocument, AppError> {
    let url = Url::parse(url).unwrap();
    f.engine
        .fetch_document(&new_id(), &scratch(), &url, auth, limits)
        .await
}

fn bearer() -> Auth {
    Auth::Bearer {
        token: "tok".into(),
    }
}

fn with_credentials() -> FetchLimits {
    FetchLimits {
        credentials: true,
        ..FetchLimits::default()
    }
}

#[tokio::test]
async fn fetch_document_gets_the_bytes_and_keeps_nothing() {
    let server = server::spawn().await;
    let f = fixture();
    let id = new_id();
    let url = Url::parse(&server.url("/echo")).unwrap();

    let document = f
        .engine
        .fetch_document(&id, &scratch(), &url, &bearer(), with_credentials())
        .await
        .unwrap();

    let echoed: Value = serde_json::from_slice(&document.bytes).unwrap();
    assert_eq!(echoed["method"], "GET");
    assert_eq!(
        echoed_header(&echoed, "authorization").as_deref(),
        Some("Bearer tok")
    );
    assert_eq!(document.final_url, url);
    assert!(f.engine.read_body(&id).is_err());
}

#[tokio::test]
async fn credentials_go_only_to_the_first_origin_and_only_when_asked() {
    let first = server::spawn().await;
    let other = server::spawn().await;
    let f = fixture();
    let same = format!("{}?url={}", first.url("/redirect-to"), first.url("/echo"));
    let cross = format!("{}?url={}", first.url("/redirect-to"), other.url("/echo"));

    let same_doc = fetch(&f, &same, &bearer(), with_credentials())
        .await
        .unwrap();
    let cross_doc = fetch(&f, &cross, &bearer(), with_credentials())
        .await
        .unwrap();
    let plain_doc = fetch(&f, &first.url("/echo"), &bearer(), FetchLimits::default())
        .await
        .unwrap();

    let header_of = |bytes: &[u8]| {
        let echoed: Value = serde_json::from_slice(bytes).unwrap();
        echoed_header(&echoed, "authorization")
    };
    assert_eq!(header_of(&same_doc.bytes).as_deref(), Some("Bearer tok"));
    assert_eq!(header_of(&cross_doc.bytes), None);
    assert_eq!(header_of(&plain_doc.bytes), None);
    assert_eq!(cross_doc.final_url.as_str(), other.url("/echo"));
}

#[tokio::test]
async fn redirects_are_checked_capped_and_stay_on_http() {
    let server = server::spawn().await;
    let f = fixture();
    let to_file = format!("{}?url=file:///etc/passwd", server.url("/redirect-to"));
    let refuse_all: HopCheck = Arc::new(|_| Err(AppError::invalid_input("hop refused")));
    let to_echo = format!("{}?url={}", server.url("/redirect-to"), server.url("/echo"));

    let file = fetch(&f, &to_file, &Auth::None, FetchLimits::default())
        .await
        .unwrap_err();
    let checked = fetch(
        &f,
        &to_echo,
        &Auth::None,
        FetchLimits {
            check_hop: Some(refuse_all),
            ..FetchLimits::default()
        },
    )
    .await
    .unwrap_err();
    let too_many = fetch(
        &f,
        &server.url("/redirect/6"),
        &Auth::None,
        FetchLimits::default(),
    )
    .await
    .unwrap_err();

    assert!(file.message.contains("not http"), "{}", file.message);
    assert_eq!(checked.message, "hop refused");
    assert_eq!(too_many.code, ErrorCode::Network);
}

#[tokio::test]
async fn a_document_over_the_cap_or_an_error_status_fails() {
    let server = server::spawn().await;
    let f = fixture();
    let small = FetchLimits {
        max_bytes: 1000,
        ..FetchLimits::default()
    };

    let big = fetch(&f, &server.url("/bytes/5000"), &Auth::None, small)
        .await
        .unwrap_err();
    let missing = fetch(
        &f,
        &server.url("/status/404"),
        &Auth::None,
        FetchLimits::default(),
    )
    .await
    .unwrap_err();

    assert!(big.message.contains("larger than"), "{}", big.message);
    assert!(missing.message.contains("404"), "{}", missing.message);
}
