//! Advanced HTTP against the local test server (plan 03b §10, `http_advanced`): Digest
//! and the forced HTTP version. Proxy, client certificates and timing are in
//! `http_network.rs`.

mod support;

use std::path::Path;

use base64::Engine as _;
use papan_lib::engine::http::types::ResponseMeta;
use papan_lib::engine::http::HttpEngine;
use papan_lib::error::{AppError, ErrorCode};
use papan_lib::store::types::{Auth, Body, HttpVersion, RequestContent, RequestLocation, Settings};
use serde_json::Value;
use support::digest::{PASSWORD, USER};
use support::server::{self, TestServer};

struct Fixture {
    _dir: tempfile::TempDir,
    engine: HttpEngine,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let engine = HttpEngine::new(&dir.path().join("cache"), dir.path(), "9.9.9").unwrap();
    Fixture { _dir: dir, engine }
}

fn scratch() -> RequestLocation {
    RequestLocation::Scratch { id: "s".into() }
}

fn digest_auth(password: &str) -> Auth {
    Auth::Digest {
        username: USER.into(),
        password: password.into(),
    }
}

fn with_digest(method: &str, url: String, password: &str) -> RequestContent {
    RequestContent {
        auth: digest_auth(password),
        ..RequestContent::new(method, url)
    }
}

/// Sends and returns the response with its body as text.
async fn send(f: &Fixture, content: &RequestContent) -> Result<(ResponseMeta, String), AppError> {
    let id = uuid::Uuid::new_v4().to_string();
    let meta = f.engine.send(&id, &scratch(), content).await?;
    let body = f.engine.read_body(&id).unwrap().data;
    Ok((meta, body))
}

fn decoded_body(body: &str) -> Vec<u8> {
    let json: Value = serde_json::from_str(body).unwrap();
    base64::engine::general_purpose::STANDARD
        .decode(json["body"].as_str().unwrap())
        .unwrap()
}

#[tokio::test]
async fn digest_succeeds_in_two_requests_for_each_algorithm() {
    let f = fixture();
    let cases = [
        ("MD5", "auth"),
        ("MD5-sess", "auth"),
        ("SHA-256", "auth"),
        ("SHA-256-sess", "auth"),
        ("MD5", "none"),
    ];

    for (algorithm, qop) in cases {
        let server = server::spawn().await;
        let url = server.url(&format!("/digest/{algorithm}/{qop}?q=1"));

        let (meta, _) = send(&f, &with_digest("GET", url, PASSWORD)).await.unwrap();

        assert_eq!(meta.status, 200, "{algorithm} {qop}");
        assert_eq!(server.hits(), 2, "{algorithm} {qop}");
    }
}

#[tokio::test]
async fn digest_replays_a_json_and_a_file_body() {
    let f = fixture();
    let server = server::spawn().await;
    let url = server.url("/digest/SHA-256/auth");
    let upload = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/http/upload.bin");
    let picked = f.engine.grant(&scratch(), &upload).unwrap();
    let json = RequestContent {
        body: Body::Json {
            text: "{\"a\":1}".into(),
        },
        ..with_digest("POST", url.clone(), PASSWORD)
    };
    let file = RequestContent {
        body: Body::Binary {
            file: Some(picked.path),
        },
        ..with_digest("PUT", url, PASSWORD)
    };

    let (json_meta, json_body) = send(&f, &json).await.unwrap();
    let (file_meta, file_body) = send(&f, &file).await.unwrap();

    assert_eq!((json_meta.status, file_meta.status), (200, 200));
    assert_eq!(decoded_body(&json_body), b"{\"a\":1}");
    assert_eq!(decoded_body(&file_body), std::fs::read(upload).unwrap());
    assert_eq!(server.hits(), 4);
}

#[tokio::test]
async fn a_basic_challenge_receives_no_credentials() {
    let f = fixture();
    let server = server::spawn().await;

    let (meta, body) = send(&f, &with_digest("GET", server.url("/basic"), PASSWORD))
        .await
        .unwrap();

    assert_eq!(meta.status, 401);
    assert_eq!(body, "", "no Authorization reached the server");
    assert_eq!(server.hits(), 1);
}

#[tokio::test]
async fn a_wrong_digest_password_returns_the_401() {
    let f = fixture();
    let server = server::spawn().await;
    let url = server.url("/digest/MD5/auth");

    let (meta, _) = send(&f, &with_digest("GET", url, "wrong")).await.unwrap();

    assert_eq!(
        (meta.status, meta.status_text.as_str()),
        (401, "Unauthorized")
    );
    assert_eq!(server.hits(), 2, "answered once, not retried");
}

#[tokio::test]
async fn an_unsupported_digest_challenge_is_returned_with_a_note() {
    let f = fixture();
    let server = server::spawn().await;
    let url = server.url("/digest/MD5/auth-int");

    let (meta, _) = send(&f, &with_digest("GET", url, PASSWORD)).await.unwrap();

    assert_eq!(meta.status, 401);
    assert_eq!(
        meta.status_text,
        "Unauthorized (Digest challenge not supported)"
    );
    assert_eq!(server.hits(), 1);
}

fn redirect_to(server: &TestServer, target: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(target.as_bytes()).collect();
    server.url(&format!("/redirect-to?url={encoded}"))
}

#[tokio::test]
async fn digest_credentials_follow_same_origin_redirects_only() {
    let f = fixture();
    let (origin, other) = (server::spawn().await, server::spawn().await);
    let same = redirect_to(&origin, "/digest/MD5/auth");
    let cross = redirect_to(&origin, &other.url("/digest/MD5/auth"));

    let (same_meta, _) = send(&f, &with_digest("GET", same, PASSWORD)).await.unwrap();
    let (cross_meta, _) = send(&f, &with_digest("GET", cross, PASSWORD))
        .await
        .unwrap();

    assert_eq!(same_meta.status, 200);
    assert_eq!(cross_meta.status, 401);
    assert_eq!(other.hits(), 1, "the other origin was never answered");
}

fn with_version(server: &TestServer, http_version: HttpVersion) -> RequestContent {
    RequestContent {
        settings: Settings {
            http_version,
            verify_tls: false,
            ..Settings::default()
        },
        ..RequestContent::new("GET", server.url("/echo"))
    }
}

#[tokio::test]
async fn the_forced_http_version_is_used() {
    let f = fixture();
    let (plain, tls) = (server::spawn().await, server::spawn_tls().await);
    let cases = [
        (&plain, HttpVersion::Auto, "HTTP/1.1"),
        (&plain, HttpVersion::Http1, "HTTP/1.1"),
        (&plain, HttpVersion::Http2, "HTTP/2.0"),
        (&tls, HttpVersion::Auto, "HTTP/2.0"),
        (&tls, HttpVersion::Http1, "HTTP/1.1"),
        (&tls, HttpVersion::Http2, "HTTP/2.0"),
    ];

    for (server, version, expected) in cases {
        let (meta, body) = send(&f, &with_version(server, version)).await.unwrap();

        let echoed: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(echoed["version"], expected, "{} {version:?}", server.scheme);
        assert_eq!(
            meta.http_version,
            expected.replace("2.0", "2"),
            "{} {version:?}",
            server.scheme
        );
    }
}

#[tokio::test]
async fn forced_http2_against_an_http1_only_server_is_a_network_error() {
    let f = fixture();
    let server = server::spawn_tls_http1().await;

    let error = send(&f, &with_version(&server, HttpVersion::Http2))
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::Network);
    assert_eq!(error.message, "The server does not support HTTP/2.");
}

#[tokio::test]
async fn the_digest_password_is_masked_in_errors() {
    let f = fixture();
    let content = with_digest("GET", "http://127.0.0.1:1/s3cr3t-pw".into(), "s3cr3t-pw");

    let error = send(&f, &content).await.unwrap_err();

    let shown = format!("{} {}", error.message, error.detail.unwrap_or_default());
    assert!(shown.contains("127.0.0.1"), "{shown}");
    assert!(!shown.contains("s3cr3t-pw"), "{shown}");
}
