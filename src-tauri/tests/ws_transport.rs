//! The WebSocket transport against local servers (plan 07 §10): connect timeout, refused
//! ports, cancel during connect, `wss` trust, client certificates and `CONNECT` proxies.

mod support;

use std::sync::Arc;
use std::time::Duration;

use papan_lib::engine::http::resolve::Resolved;
use papan_lib::engine::http::types::{ProxyInput, ProxyMode};
use papan_lib::engine::http::HttpEngine;
use papan_lib::engine::ws::handshake::connect_spec;
use papan_lib::engine::ws::types::{WsCloseBy, WsMessage, WsOpened};
use papan_lib::engine::ws::{WsEngine, WsInbound};
use papan_lib::error::{AppError, ErrorCode};
use papan_lib::store::types::{RequestContent, RequestKind, RequestLocation, Settings};
use papan_lib::vars::types::SecretEdit;
use support::proxy as test_proxy;
use support::ws_server::{self, Pki};

const PROXY_USER: &str = "alifu";
const PROXY_PASSWORD: &str = "pr0xy-pw";

struct Quiet;

impl WsInbound for Quiet {
    fn opened(&self, _opened: &WsOpened) {}
    fn message(&self, _message: &WsMessage, _raw: &[u8]) {}
    fn closed(&self, _code: Option<u32>, _reason: &str, _by: WsCloseBy, _error: Option<&AppError>) {
    }
}

struct Fixture {
    dir: tempfile::TempDir,
    http: HttpEngine,
    ws: Arc<WsEngine>,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let http = HttpEngine::new(&dir.path().join("cache"), dir.path(), "9.9.9").unwrap();
    Fixture {
        dir,
        http,
        ws: Arc::new(WsEngine::default()),
    }
}

fn cid(n: u32) -> String {
    format!("00000000-0000-4000-8000-{n:012}")
}

fn content(url: String, settings: Settings) -> RequestContent {
    RequestContent {
        kind: RequestKind::Ws,
        settings,
        ..RequestContent::new("GET", url)
    }
}

fn unverified() -> Settings {
    Settings {
        verify_tls: false,
        ..Settings::default()
    }
}

fn timeout(ms: u32) -> Settings {
    Settings {
        timeout_ms: ms,
        ..Settings::default()
    }
}

async fn connect(f: &Fixture, id: &str, content: RequestContent) -> Result<WsOpened, AppError> {
    let location = RequestLocation::Scratch { id: "s".into() };
    let spec = connect_spec(&f.http, &location, &Resolved::unchanged(content), "9.9.9").await?;
    f.ws.open(id, spec, Arc::new(Quiet)).await
}

fn use_proxy(f: &Fixture, url: String, password: &str, bypass: &[&str]) {
    let input = ProxyInput {
        mode: ProxyMode::Custom,
        url,
        username: PROXY_USER.into(),
        password: SecretEdit::Set {
            value: password.into(),
        },
        bypass: bypass.iter().map(|p| p.to_string()).collect(),
    };
    f.http.network().write_proxy(&input).unwrap();
}

#[tokio::test]
async fn a_server_that_never_answers_times_out() {
    let (f, addr) = (fixture(), ws_server::spawn_hanging().await);

    let error = connect(&f, &cid(1), content(format!("ws://{addr}/"), timeout(300)))
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::Timeout);
    assert_eq!(f.ws.open_count(), 0);
}

#[tokio::test]
async fn a_refused_port_is_a_network_error() {
    let f = fixture();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let error = connect(
        &f,
        &cid(1),
        content(format!("ws://{addr}/"), Settings::default()),
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::Network);
}

#[tokio::test]
async fn disconnect_cancels_a_connect_in_progress() {
    let (f, addr) = (fixture(), ws_server::spawn_hanging().await);
    let id = cid(1);
    let pending = {
        let (f, id) = (Arc::new(f), id.clone());
        let task = {
            let f = Arc::clone(&f);
            tokio::spawn(async move {
                connect(&f, &id, content(format!("ws://{addr}/"), timeout(0))).await
            })
        };
        while f.ws.open_count() == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        f.ws.close(&cid(1), None, "").await.unwrap();
        (task, f)
    };

    let error = pending.0.await.unwrap().unwrap_err();

    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(pending.1.ws.open_count(), 0);
}

#[tokio::test]
async fn an_untrusted_certificate_fails_unless_verification_is_off() {
    let (f, server) = (
        fixture(),
        ws_server::spawn_tls(ws_server::self_signed_config()).await,
    );

    let refused = connect(
        &f,
        &cid(1),
        content(server.wss_url("/"), Settings::default()),
    )
    .await
    .unwrap_err();
    let opened = connect(&f, &cid(2), content(server.wss_url("/"), unverified()))
        .await
        .unwrap();

    assert_eq!(refused.code, ErrorCode::Network);
    assert!(refused.message.contains("TLS"), "{}", refused.message);
    assert_eq!(opened.status, 101);
    assert!(opened.url.starts_with("wss://"));
}

#[tokio::test]
async fn a_server_that_requires_a_client_certificate_gets_the_imported_one() {
    let (f, pki) = (fixture(), Pki::new());
    let server = ws_server::spawn_tls(pki.mtls_config()).await;
    let url = server.wss_url("/");

    let without = connect(&f, &cid(1), content(url.clone(), unverified())).await;
    let pem = pki.client("papan-test");
    let (cert, key) = (
        f.dir.path().join("client.pem"),
        f.dir.path().join("client.key"),
    );
    std::fs::write(&cert, &pem.cert).unwrap();
    std::fs::write(&key, &pem.key).unwrap();
    f.http
        .network()
        .add_certificate("127.0.0.1", None, &cert, Some(&key))
        .unwrap();
    let with = connect(&f, &cid(2), content(url, unverified())).await;

    assert!(
        without.is_err(),
        "the server accepted a connection without a certificate"
    );
    assert_eq!(with.unwrap().status, 101);
}

#[tokio::test]
async fn a_connect_proxy_with_basic_auth_tunnels_ws_and_wss() {
    let (f, server) = (fixture(), ws_server::spawn().await);
    let tls = ws_server::spawn_tls(ws_server::self_signed_config()).await;
    let proxy = test_proxy::spawn(Some((PROXY_USER, PROXY_PASSWORD))).await;
    use_proxy(&f, proxy.url(), PROXY_PASSWORD, &[]);

    connect(&f, &cid(1), content(server.url("/"), Settings::default()))
        .await
        .unwrap();
    connect(&f, &cid(2), content(tls.wss_url("/"), unverified()))
        .await
        .unwrap();

    let hits = proxy.hits();
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|hit| hit.method == "CONNECT"));
    assert_eq!(hits[0].target, server.addr.to_string());
    assert!(hits[0]
        .proxy_authorization
        .as_deref()
        .unwrap()
        .starts_with("Basic "));
}

#[tokio::test]
async fn a_refused_connect_reports_the_status_without_the_password() {
    let (f, server) = (fixture(), ws_server::spawn().await);
    let proxy = test_proxy::spawn(Some((PROXY_USER, "something-else"))).await;
    use_proxy(&f, proxy.url(), PROXY_PASSWORD, &[]);

    let error = connect(&f, &cid(1), content(server.url("/"), Settings::default()))
        .await
        .unwrap_err();

    assert!(error.message.contains("407"), "{}", error.message);
    let text = format!("{error:?}");
    assert!(!text.contains(PROXY_PASSWORD), "{text}");
}

#[tokio::test]
async fn the_bypass_list_skips_the_proxy() {
    let (f, server) = (fixture(), ws_server::spawn().await);
    let proxy = test_proxy::spawn(None).await;
    use_proxy(&f, proxy.url(), PROXY_PASSWORD, &["127.0.0.1"]);

    connect(&f, &cid(1), content(server.url("/"), Settings::default()))
        .await
        .unwrap();

    assert!(proxy.hits().is_empty());
}

#[tokio::test]
async fn an_https_proxy_is_refused_with_a_clear_message() {
    let (f, server) = (fixture(), ws_server::spawn().await);
    use_proxy(&f, "https://proxy.test:3128".into(), PROXY_PASSWORD, &[]);

    let error = connect(&f, &cid(1), content(server.url("/"), Settings::default()))
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(error.message.contains("https://"), "{}", error.message);
}
