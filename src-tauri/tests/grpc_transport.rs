//! The gRPC channel through M7's transport (plan 08 §9): h2c, TLS with ALPN `h2`,
//! `verify_tls`, client certificates, `CONNECT` proxies with auth, connect timeouts,
//! refused ports and a cancel during connect.

mod support;

use std::sync::Arc;
use std::time::Duration;

use papan_lib::engine::http::types::{ProxyInput, ProxyMode};
use papan_lib::error::ErrorCode;
use papan_lib::store::types::RequestContent;
use papan_lib::vars::types::SecretEdit;
use support::grpc_server::{self, ConnectionInfo, GrpcServer, Options};
use support::grpc_world::{cid, content, world, World};
use support::mtls::TestCa;
use support::proxy as test_proxy;
use support::ws_server;

const PROXY_USER: &str = "proxy-user";
const PROXY_PASSWORD: &str = "pr0xy-Pa55";

fn echo(server: &GrpcServer, verify_tls: bool) -> RequestContent {
    let mut request = content(&server.url(), "Unary", r#"{"message":"m"}"#);
    request.settings.verify_tls = verify_tls;
    request
}

async fn status_of(w: &World, id: &str, request: &RequestContent) -> String {
    w.start(id, request).await.unwrap();
    w.finished(id, 1).await.status.name
}

fn use_proxy(w: &World, url: String, password: &str) {
    let input = ProxyInput {
        mode: ProxyMode::Custom,
        url,
        username: PROXY_USER.into(),
        password: SecretEdit::Set {
            value: password.into(),
        },
        bypass: Vec::new(),
    };
    w.http.network().write_proxy(&input).unwrap();
}

#[tokio::test]
async fn h2c_needs_no_alpn() {
    let (w, server) = (world(), grpc_server::spawn().await);

    assert_eq!(status_of(&w, &cid(1), &echo(&server, true)).await, "OK");
    let expected = ConnectionInfo {
        alpn: None,
        client_cert: false,
    };
    assert_eq!(server.connections(), [expected]);
}

#[tokio::test]
async fn a_self_signed_server_fails_verification_and_works_without_it_over_h2() {
    let (w, server) = (world(), grpc_server::spawn_tls().await);

    let refused = w.start(&cid(1), &echo(&server, true)).await.unwrap_err();
    let ok = status_of(&w, &cid(2), &echo(&server, false)).await;

    assert_eq!(refused.code, ErrorCode::Network);
    assert!(refused.message.contains("TLS"), "{refused:?}");
    assert_eq!(ok, "OK");
    let alpn: Vec<_> = server.connections().into_iter().map(|c| c.alpn).collect();
    assert_eq!(alpn, [Some(b"h2".to_vec())]);
}

#[tokio::test]
async fn a_server_that_requires_a_client_certificate_gets_the_imported_one() {
    let (w, ca) = (world(), TestCa::new());
    let server = grpc_server::spawn_with(Options {
        tls: Some(ca.server_config()),
        ..Options::default()
    })
    .await;
    let without = w.start(&cid(1), &echo(&server, false)).await;
    let without = match without {
        Err(error) => format!("{:?}", error.code),
        Ok(_) => w.finished(&cid(1), 1).await.status.name,
    };

    let pem = ca.client("papan-client");
    let (cert, key) = (w.dir.path().join("c.pem"), w.dir.path().join("c.key"));
    std::fs::write(&cert, pem.cert).unwrap();
    std::fs::write(&key, pem.key).unwrap();
    w.http
        .network()
        .add_certificate("127.0.0.1", Some(server.addr.port()), &cert, Some(&key))
        .unwrap();
    let with = status_of(&w, &cid(2), &echo(&server, false)).await;

    assert_ne!(without, "OK");
    assert_eq!(with, "OK");
    assert!(server.connections().iter().any(|c| c.client_cert));
}

#[tokio::test]
async fn a_connect_proxy_with_basic_auth_tunnels_h2c_and_tls() {
    let w = world();
    let (plain, tls) = (grpc_server::spawn().await, grpc_server::spawn_tls().await);
    let proxy = test_proxy::spawn(Some((PROXY_USER, PROXY_PASSWORD))).await;
    use_proxy(&w, proxy.url(), PROXY_PASSWORD);

    assert_eq!(status_of(&w, &cid(1), &echo(&plain, true)).await, "OK");
    assert_eq!(status_of(&w, &cid(2), &echo(&tls, false)).await, "OK");

    let hits = proxy.hits();
    let targets: Vec<String> = hits
        .iter()
        .map(|h| format!("{} {}", h.method, h.target))
        .collect();
    assert!(
        targets.contains(&format!("CONNECT {}", plain.addr)),
        "{targets:?}"
    );
    assert!(
        targets.contains(&format!("CONNECT {}", tls.addr)),
        "{targets:?}"
    );
    assert!(hits.iter().all(|h| h.proxy_authorization.is_some()));
}

#[tokio::test]
async fn a_wrong_proxy_password_fails_without_showing_it() {
    let (w, server) = (world(), grpc_server::spawn().await);
    let proxy = test_proxy::spawn(Some((PROXY_USER, "something-else"))).await;
    use_proxy(&w, proxy.url(), PROXY_PASSWORD);

    let error = w.start(&cid(1), &echo(&server, true)).await.unwrap_err();

    let shown = format!("{error:?}");
    assert_eq!(error.code, ErrorCode::Network, "{shown}");
    assert!(!shown.contains(PROXY_PASSWORD), "{shown}");
    assert!(server.calls().is_empty());
}

#[tokio::test]
async fn a_silent_server_times_out_and_a_closed_port_is_refused() {
    let w = world();
    let silent = ws_server::spawn_hanging().await;
    let mut slow = content(&format!("https://{silent}"), "Unary", "{}");
    slow.settings.timeout_ms = 200;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let closed = listener.local_addr().unwrap();
    drop(listener);

    let timed_out = w.start(&cid(1), &slow).await.unwrap_err();
    let refused = w
        .start(
            &cid(2),
            &content(&format!("http://{closed}"), "Unary", "{}"),
        )
        .await
        .unwrap_err();

    assert_eq!(timed_out.code, ErrorCode::Timeout);
    assert_eq!(refused.code, ErrorCode::Network);
    assert_eq!(w.state.engine.live_count(), 0);
}

#[tokio::test]
async fn cancel_wins_over_a_start_still_connecting() {
    let w = Arc::new(world());
    let silent = ws_server::spawn_hanging().await;
    let mut slow = content(&format!("https://{silent}"), "Unary", "{}");
    slow.settings.timeout_ms = 0;
    let task = {
        let w = Arc::clone(&w);
        tokio::spawn(async move { w.start(&cid(1), &slow).await })
    };
    while w.state.engine.live_count() == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    w.state.engine.cancel(&cid(1));
    let error = task.await.unwrap().unwrap_err();

    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(w.state.engine.live_count(), 0);
    assert!(
        w.events_of(&cid(1)).is_empty(),
        "a cancelled start emits nothing"
    );
}

#[tokio::test]
async fn the_deadline_also_covers_connecting() {
    let w = world();
    let silent = ws_server::spawn_hanging().await;
    let mut request = content(&format!("https://{silent}"), "Unary", "{}");
    request.settings.timeout_ms = 0;
    request.grpc.deadline_ms = 200;

    let started = std::time::Instant::now();
    let error = w.start(&cid(1), &request).await.unwrap_err();

    assert_eq!(error.code, ErrorCode::Timeout, "{error:?}");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(w.state.engine.live_count(), 0);
}
