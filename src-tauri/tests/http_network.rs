//! The network settings against local servers (plan 03b §10, the proxy, client
//! certificate and timing cases of `http_advanced`): a forward proxy with a hit log,
//! HTTPS servers that require a client certificate, and the four timing phases.

mod support;

use papan_lib::engine::http::types::{ProxyInput, ProxyMode, ResponseMeta};
use papan_lib::engine::http::HttpEngine;
use papan_lib::error::{AppError, ErrorCode};
use papan_lib::store::types::{RequestContent, RequestLocation, Settings};
use papan_lib::vars::types::SecretEdit;
use papan_lib::vars::vault::SecretChange;
use support::mtls::TestCa;
use support::proxy::{self as test_proxy, TestProxy};
use support::server::{self, TestServer};

const USER: &str = "alifu";

struct Fixture {
    _dir: tempfile::TempDir,
    engine: HttpEngine,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let engine = HttpEngine::new(&dir.path().join("cache"), dir.path(), "9.9.9").unwrap();
    Fixture { _dir: dir, engine }
}

/// Sends from a scratch request and returns the response with its body as text.
async fn send(f: &Fixture, content: &RequestContent) -> Result<(ResponseMeta, String), AppError> {
    let id = uuid::Uuid::new_v4().to_string();
    let location = RequestLocation::Scratch { id: "s".into() };
    let meta = f.engine.send(&id, &location, content).await?;
    let body = f.engine.read_body(&id).unwrap().data;
    Ok((meta, body))
}

fn redirect_to(server: &TestServer, target: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(target.as_bytes()).collect();
    server.url(&format!("/redirect-to?url={encoded}"))
}

// ---- proxy (plan 03b §4.1) ------------------------------------------------------------

const PROXY_PASSWORD: &str = "pr0xy-pw";

fn proxy_input(mode: ProxyMode, proxy: &TestProxy, password: SecretEdit) -> ProxyInput {
    ProxyInput {
        mode,
        url: proxy.url(),
        username: USER.into(),
        password,
        bypass: Vec::new(),
    }
}

fn use_proxy(f: &Fixture, input: &ProxyInput) {
    f.engine.network().write_proxy(input).unwrap();
}

fn unverified(url: String) -> RequestContent {
    RequestContent {
        settings: Settings {
            verify_tls: false,
            ..Settings::default()
        },
        ..RequestContent::new("GET", url)
    }
}

#[tokio::test]
async fn a_custom_proxy_is_used_and_none_or_bypass_go_direct() {
    let (f, server) = (fixture(), server::spawn().await);
    let proxy = test_proxy::spawn(None).await;
    let custom = proxy_input(ProxyMode::Custom, &proxy, SecretEdit::Keep);

    use_proxy(&f, &custom);
    let (proxied, _) = send(&f, &unverified(server.url("/echo"))).await.unwrap();
    use_proxy(
        &f,
        &ProxyInput {
            mode: ProxyMode::None,
            ..custom.clone()
        },
    );
    send(&f, &unverified(server.url("/echo"))).await.unwrap();
    use_proxy(
        &f,
        &ProxyInput {
            bypass: vec!["127.0.0.1".into()],
            ..custom
        },
    );
    send(&f, &unverified(server.url("/echo"))).await.unwrap();

    assert_eq!(proxied.status, 200);
    let hits = proxy.hits();
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].target, server.url("/echo"));
    assert_eq!(server.hits(), 3);
}

#[tokio::test]
async fn basic_proxy_auth_reaches_the_proxy_and_never_the_origin() {
    let (f, origin) = (fixture(), server::spawn_tls().await);
    let proxy = test_proxy::spawn(Some((USER, PROXY_PASSWORD))).await;
    let password = SecretEdit::Set {
        value: PROXY_PASSWORD.into(),
    };
    use_proxy(&f, &proxy_input(ProxyMode::Custom, &proxy, password));

    let (meta, body) = send(&f, &unverified(origin.url("/echo"))).await.unwrap();

    assert_eq!(meta.status, 200);
    let hits = proxy.hits();
    assert_eq!(hits[0].method, "CONNECT");
    assert!(hits[0]
        .proxy_authorization
        .as_deref()
        .is_some_and(|v| v.starts_with("Basic ")));
    assert!(
        !body.to_lowercase().contains("proxy-authorization"),
        "{body}"
    );
}

#[tokio::test]
async fn a_wrong_proxy_password_is_a_masked_error() {
    let (f, origin) = (fixture(), server::spawn_tls().await);
    let proxy = test_proxy::spawn(Some((USER, PROXY_PASSWORD))).await;
    let wrong = SecretEdit::Set {
        value: "wr0ng-pr0xy-pw".into(),
    };
    use_proxy(&f, &proxy_input(ProxyMode::Custom, &proxy, wrong));

    let error = send(&f, &unverified(origin.url("/echo")))
        .await
        .unwrap_err();

    let shown = format!("{} {}", error.message, error.detail.unwrap_or_default());
    assert_eq!(error.code, ErrorCode::Network);
    assert!(!shown.contains("wr0ng-pr0xy-pw"), "{shown}");
    assert_eq!(origin.hits(), 0);
    assert!(!proxy.hits().is_empty(), "the proxy refused it");
}

// ---- client certificates (plan 03b §4.2) ----------------------------------------------

/// Imports a client certificate of `ca` for `127.0.0.1` (and `port`).
fn add_certificate(f: &Fixture, ca: &TestCa, port: Option<u16>) -> String {
    let pem = ca.client("client.test");
    let dir = tempfile::tempdir().unwrap();
    let (cert, key) = (dir.path().join("client.pem"), dir.path().join("client.key"));
    std::fs::write(&cert, pem.cert).unwrap();
    std::fs::write(&key, pem.key).unwrap();
    let settings = f
        .engine
        .network()
        .add_certificate("127.0.0.1", port, &cert, Some(&key));
    settings.unwrap().certificates.last().unwrap().id.clone()
}

#[tokio::test]
async fn the_matching_host_presents_its_certificate_and_others_present_none() {
    let (f, ca) = (fixture(), TestCa::new());
    let (server, other) = (ca.spawn_mtls(), ca.spawn_mtls());
    add_certificate(&f, &ca, Some(server.addr.port()));

    let (meta, _) = send(&f, &unverified(server.url("/echo"))).await.unwrap();
    let refused = send(&f, &unverified(other.url("/echo"))).await.unwrap_err();

    assert_eq!(meta.status, 200);
    assert_eq!(refused.code, ErrorCode::Network);
    assert_eq!(other.hits(), 0);
}

#[tokio::test]
async fn a_redirect_to_another_mtls_server_gets_no_certificate() {
    let (f, ca) = (fixture(), TestCa::new());
    let (first, second) = (ca.spawn_mtls(), ca.spawn_mtls());
    add_certificate(&f, &ca, Some(first.addr.port()));

    let error = send(&f, &unverified(redirect_to(&first, &second.url("/echo"))))
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::Network);
    assert_eq!((first.hits(), second.hits()), (1, 0));
}

#[tokio::test]
async fn a_certificate_missing_from_the_vault_fails_the_hop() {
    let (f, ca) = (fixture(), TestCa::new());
    let server = ca.spawn_mtls();
    let id = add_certificate(&f, &ca, None);
    let delete = SecretChange::Delete {
        id: format!("n/cert/{id}"),
    };
    f.engine.network().vault().apply(&[delete]).unwrap();

    let error = send(&f, &unverified(server.url("/echo")))
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(error.message.contains("is missing"), "{}", error.message);
    assert_eq!(server.hits(), 0);
}

// ---- timing (plan 03b §6.4) -----------------------------------------------------------

fn assert_adds_up(meta: &ResponseMeta) {
    let t = meta.timing;
    assert_eq!(
        t.dns_ms + t.connect_ms + t.wait_ms + t.download_ms,
        meta.total_ms,
        "{t:?}"
    );
}

#[tokio::test]
async fn timing_adds_up_and_a_second_send_reuses_the_connection() {
    let (f, server) = (fixture(), server::spawn().await);
    let content = RequestContent::new("GET", server.url("/delay/300"));

    let (first, _) = send(&f, &content).await.unwrap();
    let (second, _) = send(&f, &content).await.unwrap();

    assert_adds_up(&first);
    assert_adds_up(&second);
    assert!(!first.timing.reused);
    assert!(second.timing.reused);
    assert!(first.timing.wait_ms >= 250, "{:?}", first.timing);
}

#[tokio::test]
async fn a_slow_body_lands_in_download() {
    let dir = tempfile::tempdir().unwrap();
    let engine = HttpEngine::new(&dir.path().join("cache"), dir.path(), "9.9.9").unwrap();
    let f = Fixture {
        _dir: dir,
        engine: engine.with_max_body_bytes(15),
    };
    let server = server::spawn().await;

    let (meta, _) = send(&f, &RequestContent::new("GET", server.url("/drip")))
        .await
        .unwrap();

    assert_adds_up(&meta);
    assert!(meta.timing.download_ms >= 250, "{:?}", meta.timing);
}
