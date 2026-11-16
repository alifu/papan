//! WebSocket mock routes (plan 11 §14): a route of kind `ws` accepts an upgrade at its
//! endpoint, sends its greeting, then answers each text message with the response the usual
//! rules and modes pick, the message standing in for the request body.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use rand::Rng;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::protocol::{Message, Role, WebSocketConfig};
use tokio_tungstenite::WebSocketStream;

use super::engine::{capped_delay, Compiled};
use super::matcher::{find_route, strip_prefix, WS_METHOD};
use super::request::RequestCtx;
use super::rules::select;
use super::server::Shared;
use super::template;
use super::types::MockLogEntry;

/// Largest message accepted from a client.
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
/// A client that sends more messages than this on one connection is disconnected.
const MAX_MESSAGES: u64 = 10_000;
/// A session ends after this long however it is used.
const MAX_SESSION: Duration = Duration::from_secs(300);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_LOGGED_MESSAGE: usize = 16 * 1024;

/// The route an upgrade matched, with the request it came in on (headers, query, params).
#[derive(Debug, Clone)]
pub struct WsPlan {
    pub index: usize,
    pub route_id: String,
    pub ctx: RequestCtx,
}

/// A message to send after `delay`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsMessage {
    pub text: String,
    pub delay: Duration,
    pub response_id: Option<String>,
}

/// The enabled WebSocket route for an upgrade request at `ctx.path`.
pub fn find(compiled: &Compiled, ctx: &RequestCtx) -> Option<WsPlan> {
    let mock = &compiled.mock;
    let relative = strip_prefix(&mock.prefix, &ctx.path)?.to_string();
    let hit = find_route(&mock.routes, compiled.routes(), WS_METHOD, &relative)?;
    let mut ctx = ctx.clone();
    ctx.params = hit.params;
    Some(WsPlan {
        index: hit.index,
        route_id: mock.routes[hit.index].id.clone(),
        ctx,
    })
}

fn render(compiled: &Compiled, source: &str, ctx: &mut RequestCtx) -> String {
    if template::uses_data(source) {
        ctx.data = compiled.buckets.snapshot();
    }
    template::render(source, ctx).unwrap_or_else(|problem| {
        tracing::warn!("mock websocket template failed: {problem}");
        serde_json::json!({ "error": format!("Template error: {problem}") }).to_string()
    })
}

/// What a client gets when it connects; `None` for a route without a greeting.
pub fn greeting(compiled: &Compiled, plan: &WsPlan) -> Option<WsMessage> {
    let route = &compiled.mock.routes[plan.index];
    if route.greeting.is_empty() {
        return None;
    }
    let mut ctx = plan.ctx.clone();
    Some(WsMessage {
        text: render(compiled, &route.greeting, &mut ctx),
        delay: Duration::ZERO,
        response_id: None,
    })
}

/// The answer to the `number`th message (from 1) of a connection; `None` when the route has no
/// responses or the chosen one is empty.
pub fn answer(compiled: &Compiled, plan: &WsPlan, text: &str, number: u64) -> Option<WsMessage> {
    let route = &compiled.mock.routes[plan.index];
    if route.responses.is_empty() {
        return None;
    }
    let mut ctx = plan.ctx.clone();
    ctx.body_text = text.to_string();
    ctx.body_json = serde_json::from_str(text).ok();
    ctx.request_number = number;
    let random = rand::rng().random_range(0..route.responses.len());
    let response = select(route, &ctx, random);
    let body = render(compiled, &response.body, &mut ctx);
    (!body.is_empty()).then(|| WsMessage {
        text: body,
        delay: capped_delay(response.latency_ms),
        response_id: Some(response.id.clone()),
    })
}

fn logged_message(
    plan: &WsPlan,
    remote: SocketAddr,
    text: &str,
    reply: Option<&WsMessage>,
) -> MockLogEntry {
    let mut cut = text.len().min(MAX_LOGGED_MESSAGE);
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    MockLogEntry {
        id: 0,
        at_ms: super::server::now_ms(),
        method: "WS".into(),
        path: plan.ctx.path.clone(),
        query: String::new(),
        headers: Vec::new(),
        body_text: text[..cut].to_string(),
        body_truncated: cut < text.len(),
        remote: remote.to_string(),
        route_id: Some(plan.route_id.clone()),
        response_id: reply.and_then(|r| r.response_id.clone()),
        status: 200,
        duration_ms: 0,
    }
}

async fn send<S>(ws: &mut WebSocketStream<S>, message: WsMessage) -> bool
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    if !message.delay.is_zero() {
        tokio::time::sleep(message.delay).await;
    }
    ws.send(Message::text(message.text)).await.is_ok()
}

async fn talk<S>(
    ws: &mut WebSocketStream<S>,
    shared: &Arc<Shared>,
    plan: &WsPlan,
    remote: SocketAddr,
) where
    S: AsyncRead + AsyncWrite + Unpin,
{
    if let Some(hello) = greeting(shared.compiled(), plan) {
        if !send(ws, hello).await {
            return;
        }
    }
    let mut number = 0_u64;
    while let Some(Ok(frame)) = ws.next().await {
        let text = match frame {
            Message::Text(text) => text.to_string(),
            Message::Close(_) => return,
            _ => continue,
        };
        number += 1;
        if number > MAX_MESSAGES {
            return;
        }
        // Templates and regex rules run off the async workers.
        let (engine, owned, body) = (Arc::clone(shared), plan.clone(), text.clone());
        let reply =
            tokio::task::spawn_blocking(move || answer(engine.compiled(), &owned, &body, number))
                .await
                .unwrap_or(None);
        shared.record(logged_message(plan, remote, &text, reply.as_ref()));
        if let Some(reply) = reply {
            if !send(ws, reply).await {
                return;
            }
        }
    }
}

/// Runs one upgraded connection until the client leaves, the mock stops or time runs out.
pub async fn session<S>(
    io: S,
    shared: Arc<Shared>,
    plan: WsPlan,
    remote: SocketAddr,
    mut stopped: watch::Receiver<bool>,
) where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_MESSAGE_BYTES));
    let mut ws = WebSocketStream::from_raw_socket(io, Role::Server, Some(config)).await;
    tokio::select! {
        () = talk(&mut ws, &shared, &plan, remote) => {}
        _ = stopped.changed() => {}
        () = tokio::time::sleep(MAX_SESSION) => {}
    }
    let _ = tokio::time::timeout(CLOSE_TIMEOUT, ws.close(None)).await;
}
