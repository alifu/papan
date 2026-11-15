//! GraphQL subscription servers for the M6b tests (plan 06b §9): a real `async-graphql`
//! schema on axum (`/ws`, token checked in `connection_init`, `Authorization` and init
//! payloads recorded) and a scripted `graphql-transport-ws` peer for misbehaviour.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_graphql::http::ALL_WEBSOCKET_PROTOCOLS;
use async_graphql::{Data, Object, Schema, Subscription};
use async_graphql_axum::{GraphQLProtocol, GraphQLWebSocket};
use axum::extract::{Request, State, WebSocketUpgrade};
use axum::http::header;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, Stream, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::handshake::server::{
    Request as HsRequest, Response as HsResponse,
};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;

pub const TOKEN: &str = "init-token-5521";
pub const SUBPROTOCOL: &str = "graphql-transport-ws";

pub struct Query;

#[Object]
impl Query {
    async fn ok(&self) -> bool {
        true
    }
}

/// Counts streams that were dropped: a stopped or closed subscription drops its stream.
#[derive(Default)]
struct Dropped(Arc<AtomicUsize>);

struct Guard(Arc<AtomicUsize>);

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

pub struct Subs;

#[Subscription]
impl Subs {
    /// `count` numbers, one per `delay_ms`.
    async fn ticks(
        &self,
        ctx: &async_graphql::Context<'_>,
        count: i32,
        delay_ms: Option<u64>,
    ) -> impl Stream<Item = i32> {
        let guard = Guard(Arc::clone(&ctx.data_unchecked::<Dropped>().0));
        let delay = Duration::from_millis(delay_ms.unwrap_or(0));
        futures_util::stream::unfold(0, move |n| {
            let _keep = &guard;
            async move {
                if n >= count {
                    return None;
                }
                tokio::time::sleep(delay).await;
                Some((n, n + 1))
            }
        })
    }
}

type Gql = Schema<Query, async_graphql::EmptyMutation, Subs>;

#[derive(Default)]
struct Seen {
    authorizations: Mutex<Vec<Option<String>>>,
    inits: Mutex<Vec<serde_json::Value>>,
}

#[derive(Clone)]
struct App {
    schema: Gql,
    seen: Arc<Seen>,
}

pub struct GqlServer {
    pub addr: SocketAddr,
    seen: Arc<Seen>,
    dropped: Arc<AtomicUsize>,
}

impl GqlServer {
    pub fn url(&self) -> String {
        format!("http://{}/ws", self.addr)
    }

    pub fn authorizations(&self) -> Vec<Option<String>> {
        self.seen.authorizations.lock().unwrap().clone()
    }

    pub fn inits(&self) -> Vec<serde_json::Value> {
        self.seen.inits.lock().unwrap().clone()
    }

    /// Subscription streams the server has finished with (completed, stopped or closed).
    pub fn dropped_streams(&self) -> usize {
        self.dropped.load(Ordering::SeqCst)
    }
}

async fn record(State(app): State<App>, request: Request, next: Next) -> Response {
    let header = request
        .headers()
        .get(header::AUTHORIZATION)
        .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned());
    app.seen.authorizations.lock().unwrap().push(header);
    next.run(request).await
}

async fn upgrade(
    State(app): State<App>,
    protocol: GraphQLProtocol,
    upgrade: WebSocketUpgrade,
) -> Response {
    let seen = Arc::clone(&app.seen);
    upgrade
        .protocols(ALL_WEBSOCKET_PROTOCOLS)
        .on_upgrade(move |socket| {
            GraphQLWebSocket::new(socket, app.schema, protocol)
                .on_connection_init(move |payload| async move {
                    seen.inits.lock().unwrap().push(payload.clone());
                    let token = payload.get("token").and_then(|t| t.as_str());
                    if token != Some(TOKEN) {
                        return Err("Forbidden".into());
                    }
                    Ok(Data::default())
                })
                .serve()
        })
}

/// A real schema; `connection_init` must carry `{"token": TOKEN}`.
pub async fn spawn_schema() -> GqlServer {
    let dropped = Arc::new(AtomicUsize::new(0));
    let schema = Schema::build(Query, async_graphql::EmptyMutation, Subs)
        .data(Dropped(Arc::clone(&dropped)))
        .finish();
    let app = App {
        schema,
        seen: Arc::default(),
    };
    let seen = Arc::clone(&app.seen);
    let router = Router::new()
        .route("/ws", get(upgrade))
        .layer(middleware::from_fn_with_state(app.clone(), record))
        .with_state(app);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    GqlServer {
        addr,
        seen,
        dropped,
    }
}

/// How the scripted peer misbehaves.
#[derive(Debug, Clone, Copy)]
pub enum Script {
    /// Never acknowledges the connection.
    NoAck,
    /// Acknowledges, then pings after `subscribe` and sends one `next`.
    Ping,
    /// Closes with this code on `connection_init`.
    CloseOnInit(u16),
    /// Acknowledges, sends one `next` after `subscribe`, then waits.
    Stream,
}

pub struct ScriptedServer {
    pub addr: SocketAddr,
    received: Arc<Mutex<Vec<String>>>,
}

impl ScriptedServer {
    pub fn url(&self) -> String {
        format!("ws://{}/", self.addr)
    }

    /// Text frames received so far; `"<close>"` marks a close frame.
    pub fn received(&self) -> Vec<String> {
        self.received.lock().unwrap().clone()
    }

    pub fn types(&self) -> Vec<String> {
        self.received()
            .iter()
            .map(|text| {
                serde_json::from_str::<serde_json::Value>(text)
                    .ok()
                    .and_then(|v| v.get("type").and_then(|t| t.as_str().map(String::from)))
                    .unwrap_or_else(|| text.clone())
            })
            .collect()
    }
}

pub async fn spawn_script(script: Script) -> ScriptedServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let received: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = Arc::clone(&received);
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(serve(stream, script, Arc::clone(&log)));
        }
    });
    ScriptedServer { addr, received }
}

#[allow(clippy::result_large_err)] // the handshake callback's error type is fixed by tungstenite
fn pick_protocol(
    request: &HsRequest,
    mut response: HsResponse,
) -> Result<HsResponse, tokio_tungstenite::tungstenite::http::Response<Option<String>>> {
    if request.headers().get("sec-websocket-protocol").is_some() {
        response
            .headers_mut()
            .insert("sec-websocket-protocol", SUBPROTOCOL.parse().unwrap());
    }
    Ok(response)
}

fn text_of(value: &serde_json::Value) -> Message {
    Message::text(value.to_string())
}

async fn serve(stream: tokio::net::TcpStream, script: Script, log: Arc<Mutex<Vec<String>>>) {
    let Ok(mut socket) = accept_hdr_async(stream, pick_protocol).await else {
        return;
    };
    while let Some(Ok(message)) = socket.next().await {
        let text = match message {
            Message::Text(text) => text.to_string(),
            Message::Close(_) => {
                log.lock().unwrap().push("<close>".into());
                return;
            }
            _ => continue,
        };
        log.lock().unwrap().push(text.clone());
        let value: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let kind = value.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let id = value.get("id").cloned().unwrap_or_default();
        let replies = match (script, kind) {
            (Script::CloseOnInit(code), "connection_init") => {
                let frame = CloseFrame {
                    code: CloseCode::from(code),
                    reason: "scripted close".into(),
                };
                vec![Message::Close(Some(frame))]
            }
            (Script::NoAck, _) => Vec::new(),
            (_, "connection_init") => vec![text_of(&serde_json::json!({"type": "connection_ack"}))],
            (Script::Ping, "subscribe") => vec![
                text_of(&serde_json::json!({"type": "ping", "payload": {"n": 1}})),
                text_of(
                    &serde_json::json!({"id": id, "type": "next", "payload": {"data": {"ticks": 0}}}),
                ),
            ],
            (Script::Stream, "subscribe") => vec![text_of(
                &serde_json::json!({"id": id, "type": "next", "payload": {"data": {"ticks": 0}}}),
            )],
            _ => Vec::new(),
        };
        for reply in replies {
            if socket.send(reply).await.is_err() {
                return;
            }
        }
    }
}
