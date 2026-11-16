//! The WebSocket upgrade of a mock: handshake checks, the `Origin` rule and the hand-over of the
//! connection to a session (plan 11 §14).

use std::net::SocketAddr;
use std::sync::Arc;

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::Response;
use hyper_util::rt::TokioIo;

use super::model::MockEnvironment;
use super::request::RequestCtx;
use super::server::{is_loopback_host, now_ms, plain, Shared, Upgrades};
use super::types::MockLogEntry;
use super::ws;

fn header_has(parts: &http::request::Parts, name: &str, word: &str) -> bool {
    parts
        .headers
        .get_all(name)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .any(|token| token.trim().eq_ignore_ascii_case(word))
}

pub(super) fn wants_websocket(parts: &http::request::Parts) -> bool {
    parts.method == http::Method::GET
        && header_has(parts, "upgrade", "websocket")
        && header_has(parts, "connection", "upgrade")
}

/// WebSockets have no CORS, so a mock decides by the `Origin` header. Pages of the mock's own
/// origin are let in, and so are pages on this machine when CORS is on; any other site is not, and
/// neither is a request whose `Origin` is repeated or unreadable.
pub(super) fn origin_refused(
    mock: &MockEnvironment,
    parts: &http::request::Parts,
    ctx: &RequestCtx,
) -> bool {
    let mut values = parts.headers.get_all("origin").iter();
    let Some(first) = values.next() else {
        return false;
    };
    let Ok(origin) = first.to_str() else {
        return true;
    };
    if values.next().is_some() {
        return true;
    }
    let authority = origin.split_once("://").map_or(origin, |(_, rest)| rest);
    if authority.eq_ignore_ascii_case(&ctx.host) {
        return false;
    }
    !(mock.cors && is_loopback_host(authority))
}

/// Completes the WebSocket handshake for a matched route and hands the connection to a session.
pub(super) fn accept_websocket(
    plan: ws::WsPlan,
    parts: &http::request::Parts,
    upgrade: hyper::upgrade::OnUpgrade,
    shared: &Arc<Shared>,
    remote: SocketAddr,
    upgrades: Upgrades,
) -> Response<Full<Bytes>> {
    let version_ok = parts
        .headers
        .get("sec-websocket-version")
        .is_some_and(|v| v.as_bytes() == b"13");
    let Some(key) = parts
        .headers
        .get("sec-websocket-key")
        .filter(|_| version_ok)
    else {
        let mut response = plain(400, "Bad WebSocket handshake.");
        response.headers_mut().insert(
            "Sec-WebSocket-Version",
            http::HeaderValue::from_static("13"),
        );
        return response;
    };
    let accept = tokio_tungstenite::tungstenite::handshake::derive_accept_key(key.as_bytes());
    shared.record(MockLogEntry {
        id: 0,
        at_ms: now_ms(),
        method: "GET".into(),
        path: plan.ctx.path.clone(),
        query: parts.uri.query().unwrap_or("").to_string(),
        headers: Vec::new(),
        body_text: String::new(),
        body_truncated: false,
        remote: remote.to_string(),
        route_id: Some(plan.route_id.clone()),
        response_id: None,
        status: 101,
        duration_ms: 0,
    });
    let engine = Arc::clone(shared);
    tokio::spawn(async move {
        match upgrade.await {
            Ok(upgraded) => {
                ws::session(
                    TokioIo::new(upgraded),
                    engine,
                    plan,
                    remote,
                    upgrades.stopped.clone(),
                )
                .await;
            }
            Err(source) => tracing::debug!(%source, "websocket upgrade failed"),
        }
        drop(upgrades.guard);
    });
    let mut response = Response::new(Full::new(Bytes::new()));
    *response.status_mut() = http::StatusCode::SWITCHING_PROTOCOLS;
    let headers = response.headers_mut();
    headers.insert("Upgrade", http::HeaderValue::from_static("websocket"));
    headers.insert("Connection", http::HeaderValue::from_static("Upgrade"));
    if let Ok(value) = http::HeaderValue::from_str(&accept) {
        headers.insert("Sec-WebSocket-Accept", value);
    }
    response
}
