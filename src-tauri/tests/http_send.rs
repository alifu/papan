//! The HTTP engine against the local test server (plan 03 §10, `http_send`).

mod support;

use std::path::PathBuf;

use base64::Engine as _;
use papan_lib::engine::http::service::record_history;
use papan_lib::engine::http::types::ResponseMeta;
use papan_lib::engine::http::HttpEngine;
use papan_lib::error::{AppError, ErrorCode};
use papan_lib::store::types::{
    ApiKeyPlacement, Auth, Body, KeyValue, Part, RequestContent, RequestLocation, Settings,
};
use papan_lib::store::{Store, StoreState};
use serde_json::Value;
use support::server::{self, fixture_dir, TestServer};

struct Fixture {
    dir: tempfile::TempDir,
    engine: HttpEngine,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let engine = HttpEngine::new(&dir.path().join("cache"), dir.path(), "9.9.9").unwrap();
    Fixture { dir, engine }
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn scratch() -> RequestLocation {
    RequestLocation::Scratch { id: "s".into() }
}

fn collection(id: &str) -> RequestLocation {
    RequestLocation::Collection {
        collection_id: id.into(),
        path: "r.http.yaml".into(),
    }
}

fn get(url: &str) -> RequestContent {
    RequestContent::new("GET", url)
}

fn post(server: &TestServer, body: Body) -> RequestContent {
    RequestContent {
        body,
        ..RequestContent::new("POST", server.url("/echo"))
    }
}

async fn send(f: &Fixture, content: &RequestContent) -> Result<ResponseMeta, AppError> {
    f.engine.send(&new_id(), &scratch(), content).await
}

/// Sends and returns the response body as text.
async fn text_of(f: &Fixture, location: &RequestLocation, content: &RequestContent) -> String {
    let id = new_id();
    f.engine.send(&id, location, content).await.unwrap();
    f.engine.read_body(&id).unwrap().data
}

async fn echo(f: &Fixture, content: &RequestContent) -> Value {
    serde_json::from_str(&text_of(f, &scratch(), content).await).unwrap()
}

fn echoed_header(echo: &Value, name: &str) -> Option<String> {
    echo["headers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str().unwrap().to_string())
}

fn echoed_body(echo: &Value) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(echo["body"].as_str().unwrap())
        .unwrap()
}

fn upload_bin() -> PathBuf {
    fixture_dir().join("upload.bin")
}

#[tokio::test(flavor = "multi_thread")]
async fn text_bodies_arrive_intact_with_their_content_type() {
    let server = server::spawn().await;
    let f = fixture();
    let cases = [
        (
            Body::Json {
                text: "{\"a\":1}".into(),
            },
            "{\"a\":1}",
            "application/json",
        ),
        (
            Body::Xml {
                text: "<a/>".into(),
            },
            "<a/>",
            "application/xml",
        ),
        (
            Body::Text {
                text: "héllo".into(),
            },
            "héllo",
            "text/plain; charset=utf-8",
        ),
    ];

    for (body, text, content_type) in cases {
        let echo = echo(&f, &post(&server, body)).await;
        assert_eq!(echoed_body(&echo), text.as_bytes());
        assert_eq!(
            echoed_header(&echo, "content-type").as_deref(),
            Some(content_type)
        );
        assert_eq!(
            echoed_header(&echo, "user-agent").as_deref(),
            Some("Papan/9.9.9")
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn form_bodies_are_url_encoded() {
    let server = server::spawn().await;
    let f = fixture();
    let fields = vec![KeyValue {
        name: "a b".into(),
        value: "c&d".into(),
        enabled: true,
    }];

    let echo = echo(&f, &post(&server, Body::Form { fields })).await;

    assert_eq!(echoed_body(&echo), b"a+b=c%26d");
}

#[tokio::test(flavor = "multi_thread")]
async fn granted_files_upload_byte_exact_as_binary_and_multipart() {
    let server = server::spawn().await;
    let f = fixture();
    let picked = f.engine.grant(&scratch(), &upload_bin()).unwrap();
    let expected = std::fs::read(upload_bin()).unwrap();
    let parts = vec![
        Part {
            name: "note".into(),
            value: "hi".into(),
            file: None,
            enabled: true,
        },
        Part {
            name: "upload".into(),
            value: String::new(),
            file: Some(picked.path.clone()),
            enabled: true,
        },
    ];

    let binary = echo(
        &f,
        &post(
            &server,
            Body::Binary {
                file: Some(picked.path.clone()),
            },
        ),
    )
    .await;
    let multipart = echo(&f, &post(&server, Body::Multipart { parts })).await;

    assert_eq!(picked.size_bytes, 256);
    assert_eq!(echoed_body(&binary), expected);
    let file_part = &multipart["parts"][1];
    assert_eq!(multipart["parts"][0]["name"], "note");
    assert_eq!(file_part["file_name"], "upload.bin");
    assert_eq!(file_part["size"], 256);
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(file_part["body"].as_str().unwrap())
            .unwrap(),
        expected
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ungranted_file_is_refused_before_anything_is_sent() {
    let server = server::spawn().await;
    let f = fixture();
    let planted = f.dir.path().join("id_ed25519");
    std::fs::write(&planted, b"private").unwrap();
    let content = post(
        &server,
        Body::Binary {
            file: Some(planted.display().to_string()),
        },
    );

    let error = send(&f, &content).await.unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(error.message.contains("id_ed25519"), "{}", error.message);
    assert_eq!(server.hits(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_granted_to_one_collection_is_refused_in_another() {
    let server = server::spawn().await;
    let f = fixture();
    let picked = f.engine.grant(&collection("mine"), &upload_bin()).unwrap();
    let content = post(
        &server,
        Body::Binary {
            file: Some(picked.path.clone()),
        },
    );

    let planted = f
        .engine
        .send(&new_id(), &collection("hostile"), &content)
        .await;
    f.engine
        .carry_grants(&collection("mine"), &collection("hostile"))
        .unwrap();
    let carried = f
        .engine
        .send(&new_id(), &collection("hostile"), &content)
        .await;

    assert_eq!(planted.unwrap_err().code, ErrorCode::InvalidInput);
    assert_eq!(carried.unwrap().status, 200);
    assert_eq!(server.hits(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_id_cannot_be_reused() {
    let server = server::spawn().await;
    let f = fixture();
    let id = new_id();
    let content = get(&server.url("/status/200"));

    f.engine.send(&id, &scratch(), &content).await.unwrap();
    let again = f.engine.send(&id, &scratch(), &content).await.unwrap_err();

    assert_eq!(again.code, ErrorCode::InvalidInput);
}

#[tokio::test(flavor = "multi_thread")]
async fn each_auth_type_is_sent() {
    let server = server::spawn().await;
    let f = fixture();
    let with_auth = |auth| RequestContent {
        auth,
        ..get(&server.url("/echo?x=1"))
    };

    let basic = echo(
        &f,
        &with_auth(Auth::Basic {
            username: "u".into(),
            password: "p".into(),
        }),
    )
    .await;
    let bearer = echo(&f, &with_auth(Auth::Bearer { token: "t".into() })).await;
    let header = echo(
        &f,
        &with_auth(Auth::ApiKey {
            name: "X-Key".into(),
            value: "k".into(),
            placement: ApiKeyPlacement::Header,
        }),
    )
    .await;
    let query = echo(
        &f,
        &with_auth(Auth::ApiKey {
            name: "key".into(),
            value: "k 1".into(),
            placement: ApiKeyPlacement::Query,
        }),
    )
    .await;

    assert_eq!(
        echoed_header(&basic, "authorization").as_deref(),
        Some("Basic dTpw")
    );
    assert_eq!(
        echoed_header(&bearer, "authorization").as_deref(),
        Some("Bearer t")
    );
    assert_eq!(echoed_header(&header, "x-key").as_deref(), Some("k"));
    assert_eq!(query["query"], "x=1&key=k+1");
}

#[tokio::test(flavor = "multi_thread")]
async fn get_with_a_body_sends_it() {
    let server = server::spawn().await;
    let f = fixture();
    let content = RequestContent {
        body: Body::Text { text: "q".into() },
        ..get(&server.url("/echo"))
    };

    let echo = echo(&f, &content).await;

    assert_eq!(echo["method"], "GET");
    assert_eq!(echoed_body(&echo), b"q");
}

#[tokio::test(flavor = "multi_thread")]
async fn error_statuses_are_responses_and_gzip_is_decoded() {
    let server = server::spawn().await;
    let f = fixture();

    for code in [204u16, 404, 500] {
        let meta = send(&f, &get(&server.url(&format!("/status/{code}"))))
            .await
            .unwrap();
        assert_eq!(meta.status, code);
    }
    let unzipped = text_of(&f, &scratch(), &get(&server.url("/gzip"))).await;

    assert_eq!(unzipped, "unzipped text");
}

#[tokio::test(flavor = "multi_thread")]
async fn redirects_are_followed_or_returned() {
    let server = server::spawn().await;
    let f = fixture();
    let no_follow = RequestContent {
        settings: Settings {
            follow_redirects: false,
            ..Settings::default()
        },
        ..get(&server.url("/redirect/2"))
    };

    let followed = send(&f, &get(&server.url("/redirect/2"))).await.unwrap();
    let returned = send(&f, &no_follow).await.unwrap();

    assert_eq!(followed.status, 200);
    assert_eq!(followed.url, server.url("/redirect/0"));
    assert_eq!(returned.status, 302);
    assert!(!followed.tls_verified);
}

fn secret_headers() -> Vec<KeyValue> {
    ["X-Api-Key", "X-Auth-Token"]
        .into_iter()
        .map(|name| KeyValue {
            name: name.into(),
            value: "typed-secret".into(),
            enabled: true,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn secrets_are_not_forwarded_to_another_origin() {
    let first = server::spawn().await;
    let second = server::spawn().await;
    let f = fixture();
    let redirect = first.url(&format!("/redirect-to?url={}", second.url("/echo")));
    let bearer = RequestContent {
        auth: Auth::Bearer { token: "t".into() },
        headers: secret_headers(),
        ..get(&redirect)
    };
    let api_key = RequestContent {
        auth: Auth::ApiKey {
            name: "X-Custom-Key".into(),
            value: "k".into(),
            placement: ApiKeyPlacement::Header,
        },
        ..get(&redirect)
    };

    let after_bearer = echo(&f, &bearer).await;
    let after_api_key = echo(&f, &api_key).await;

    for name in ["authorization", "x-api-key", "x-auth-token", "x-custom-key"] {
        assert_eq!(echoed_header(&after_bearer, name), None, "{name}");
        assert_eq!(echoed_header(&after_api_key, name), None, "{name}");
    }
    assert_eq!(second.hits(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn same_origin_redirects_keep_secrets_and_post_becomes_get_on_302() {
    let server = server::spawn().await;
    let f = fixture();
    let content = RequestContent {
        auth: Auth::Bearer { token: "t".into() },
        body: Body::Text {
            text: "dropped".into(),
        },
        ..RequestContent::new("POST", server.url("/redirect-to?url=/echo"))
    };

    let echo = echo(&f, &content).await;

    assert_eq!(echo["method"], "GET");
    assert_eq!(echoed_body(&echo), b"");
    assert_eq!(
        echoed_header(&echo, "authorization").as_deref(),
        Some("Bearer t")
    );
    assert_eq!(echoed_header(&echo, "content-type"), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn more_than_ten_redirects_fail() {
    let server = server::spawn().await;
    let f = fixture();

    let ten = send(&f, &get(&server.url("/redirect/10"))).await.unwrap();
    let eleven = send(&f, &get(&server.url("/redirect/11")))
        .await
        .unwrap_err();

    assert_eq!(ten.status, 200);
    assert_eq!(eleven.code, ErrorCode::Network);
}

#[tokio::test(flavor = "multi_thread")]
async fn failure_details_never_contain_the_query_string() {
    let server = server::spawn().await;
    let f = fixture();
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let with_key = |url: String| RequestContent {
        auth: Auth::ApiKey {
            name: "key".into(),
            value: "sekret-value".into(),
            placement: ApiKeyPlacement::Query,
        },
        settings: Settings {
            timeout_ms: 200,
            ..Settings::default()
        },
        ..get(&format!("{url}?token=sekret-token"))
    };

    let refused = send(&f, &with_key(format!("http://127.0.0.1:{port}/p")))
        .await
        .unwrap_err();
    let timed_out = send(&f, &with_key(server.url("/delay/3000")))
        .await
        .unwrap_err();

    for error in [refused, timed_out] {
        let text = format!("{} {:?}", error.message, error.detail);
        assert!(!text.contains("sekret"), "{text}");
        assert!(error.detail.unwrap().contains("127.0.0.1"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn timeouts_and_refused_connections_have_their_own_codes() {
    let server = server::spawn().await;
    let f = fixture();
    let slow = RequestContent {
        settings: Settings {
            timeout_ms: 200,
            ..Settings::default()
        },
        ..get(&server.url("/delay/3000"))
    };
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);

    let timed_out = send(&f, &slow).await.unwrap_err();
    let refused = send(&f, &get(&format!("http://127.0.0.1:{port}/")))
        .await
        .unwrap_err();

    assert_eq!(timed_out.code, ErrorCode::Timeout);
    assert_eq!(refused.code, ErrorCode::Network);
    assert!(refused.message.contains("127.0.0.1"), "{}", refused.message);
}

#[tokio::test(flavor = "multi_thread")]
async fn cookies_stay_in_their_jar_until_cleared() {
    let server = server::spawn().await;
    let f = fixture();
    let read = get(&server.url("/cookies"));

    let set = f
        .engine
        .send(&new_id(), &scratch(), &get(&server.url("/cookies/set?a=1")))
        .await
        .unwrap();
    let same_jar = text_of(&f, &RequestLocation::Scratch { id: "t".into() }, &read).await;
    let other_jar = text_of(&f, &collection("c1"), &read).await;
    f.engine.clear_cookies(&scratch());
    let after_clear = text_of(&f, &scratch(), &read).await;

    assert_eq!(set.cookies[0].name, "a");
    assert!(set.cookies[0].http_only);
    assert_eq!(
        set.cookies[0].expires.as_deref(),
        Some("Wed, 21 Oct 2099 07:28:00 GMT")
    );
    assert_eq!(same_jar, "a=1");
    assert_eq!(other_jar, "");
    assert_eq!(after_clear, "");
}

#[tokio::test(flavor = "multi_thread")]
async fn unverified_tls_uses_a_separate_jar() {
    let server = server::spawn().await;
    let f = fixture();
    let unverified = |path: &str| RequestContent {
        settings: Settings {
            verify_tls: false,
            ..Settings::default()
        },
        ..get(&server.url(path))
    };
    let read = get(&server.url("/cookies"));

    f.engine
        .send(&new_id(), &scratch(), &unverified("/cookies/set?u=1"))
        .await
        .unwrap();
    let verified = text_of(&f, &scratch(), &read).await;
    let same_mode = text_of(&f, &scratch(), &unverified("/cookies")).await;
    f.engine.clear_cookies(&scratch());
    let cleared = text_of(&f, &scratch(), &unverified("/cookies")).await;

    assert_eq!(verified, "");
    assert_eq!(same_mode, "u=1");
    assert_eq!(cleared, "");
}

#[tokio::test(flavor = "multi_thread")]
async fn self_signed_https_needs_verification_turned_off() {
    let server = server::spawn_tls().await;
    let f = fixture();
    let unverified = RequestContent {
        settings: Settings {
            verify_tls: false,
            ..Settings::default()
        },
        ..get(&server.url("/status/200"))
    };

    let refused = send(&f, &get(&server.url("/status/200")))
        .await
        .unwrap_err();
    let accepted = send(&f, &unverified).await.unwrap();

    assert_eq!(refused.code, ErrorCode::Network);
    assert!(
        refused.message.contains("certificate"),
        "{}",
        refused.message
    );
    assert_eq!(accepted.status, 200);
    assert!(!accepted.tls_verified);
}

#[tokio::test(flavor = "multi_thread")]
async fn sends_are_recorded_in_history_except_cancelled_ones() {
    let server = server::spawn().await;
    let f = fixture();
    let state = StoreState::new(Store::new(&f.dir.path().join("data")).unwrap());
    let content = get(&server.url("/status/201"));
    let ok = send(&f, &content).await;
    let failed: Result<ResponseMeta, AppError> = Err(AppError::invalid_input("x"));
    let cancelled = Err(AppError::new(ErrorCode::Cancelled, "c"));
    let elapsed = std::time::Duration::from_millis(4);

    for result in [&ok, &failed, &cancelled] {
        record_history(&state, &scratch(), &content, 1.0, elapsed, result);
    }
    let listed = state.lock_store().list_history();

    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].error_code, Some(ErrorCode::InvalidInput));
    assert_eq!(
        (listed[1].status, listed[1].name.as_str()),
        (Some(201), "Untitled")
    );
}
