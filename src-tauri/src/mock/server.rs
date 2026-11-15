//! The HTTP/1 listener for one running mock (plan 11 §5): the same accept-loop shape as
//! the webhook listener, answering from the mock engine.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio::sync::{watch, OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinSet;

use super::engine::{failed_reply, respond, Compiled, PreparedCallback, Reply};
use super::model::{MockBind, MockEnvironment};
use super::request::{parse_cookies, parse_pairs, RequestCtx};
use super::types::MockLogEntry;
use super::MockSink;
use super::{files, proxy, tls, ws};
use crate::engine::http::oauth::listener::bind_exclusive;
use crate::error::AppError;
use crate::store::types::KeyValue;
use crate::webhook::server::{collect_body, BodyRead};

const HEADER_TIMEOUT: Duration = Duration::from_secs(10);
const BODY_TIMEOUT: Duration = Duration::from_secs(30);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_HEADERS: usize = 100;
const MAX_HEAD_BYTES: usize = 64 * 1024;
const MAX_CONNECTIONS: usize = 32;
/// One client cannot hold more than this many of the connections.
const MAX_PER_CLIENT: usize = 8;
/// A connection is closed after this long however it is used (slow readers, endless keep-alive).
const MAX_CONNECTION_LIFETIME: Duration = Duration::from_secs(300);
/// Requests larger than this get a 413 and are not matched.
pub const MAX_REQUEST_BODY: usize = 1024 * 1024;
/// Body text kept in the log per request.
const MAX_LOGGED_BODY: usize = 16 * 1024;
pub const MAX_LOG_ENTRIES: usize = 500;

const CALLBACK_TIMEOUT: Duration = Duration::from_secs(10);
/// Callbacks past this many in flight are dropped, so a request flood cannot make the mock
/// a flood of outgoing requests.
const MAX_CALLBACKS_IN_FLIGHT: usize = 8;

fn has_callbacks(mock: &MockEnvironment) -> bool {
    mock.routes
        .iter()
        .flat_map(|route| &route.responses)
        .chain(mock.fallback.as_ref())
        .any(|response| !response.callbacks.is_empty())
}

/// What one running mock has served; shared with the connection tasks.
pub struct Shared {
    collection_id: String,
    file: String,
    compiled: Compiled,
    root: PathBuf,
    upstream: Option<reqwest::Client>,
    callback_client: Option<reqwest::Client>,
    callback_slots: Arc<Semaphore>,
    closed: std::sync::atomic::AtomicBool,
    log: Mutex<VecDeque<MockLogEntry>>,
    next_id: AtomicU32,
    sink: Arc<dyn MockSink>,
}

impl Shared {
    pub fn new(
        collection_id: &str,
        file: &str,
        root: PathBuf,
        mock: MockEnvironment,
        sink: Arc<dyn MockSink>,
    ) -> Self {
        let callback_client = if has_callbacks(&mock) {
            proxy::client_with_timeout(CALLBACK_TIMEOUT).ok()
        } else {
            None
        };
        let upstream = if mock.proxy.is_empty() {
            None
        } else {
            proxy::client().ok()
        };
        Self {
            collection_id: collection_id.to_string(),
            file: file.to_string(),
            compiled: Compiled::new(mock),
            root,
            upstream,
            callback_client,
            callback_slots: Arc::new(Semaphore::new(MAX_CALLBACKS_IN_FLIGHT)),
            closed: std::sync::atomic::AtomicBool::new(false),
            log: Mutex::new(VecDeque::new()),
            next_id: AtomicU32::new(1),
            sink,
        }
    }

    /// Sends the callbacks of an answer in the background; none are sent once the mock stopped.
    pub fn send_callbacks(self: &Arc<Self>, callbacks: Vec<PreparedCallback>) {
        let Some(client) = &self.callback_client else {
            return;
        };
        for callback in callbacks {
            let Ok(slot) = Arc::clone(&self.callback_slots).try_acquire_owned() else {
                tracing::warn!(mock = %self.file, "too many callbacks in flight; one dropped");
                continue;
            };
            let (shared, client) = (Arc::clone(self), client.clone());
            tokio::spawn(async move {
                tokio::time::sleep(callback.delay).await;
                if !shared.closed.load(Ordering::Relaxed) {
                    send_callback(&client, &callback, &shared.file).await;
                }
                drop(slot);
            });
        }
    }

    pub fn is_tls(&self) -> bool {
        self.compiled.mock.tls
    }

    pub fn log(&self) -> MutexGuard<'_, VecDeque<MockLogEntry>> {
        self.log.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn compiled(&self) -> &Compiled {
        &self.compiled
    }

    pub(super) fn record(&self, mut entry: MockLogEntry) {
        entry.id = self.next_id.fetch_add(1, Ordering::Relaxed);
        {
            let mut log = self.log();
            if log.len() >= MAX_LOG_ENTRIES {
                log.pop_front();
            }
            log.push_back(entry.clone());
        }
        self.sink.logged(&self.collection_id, &self.file, &entry);
    }
}

pub struct RunningServer {
    pub port: u16,
    shared: Arc<Shared>,
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

impl RunningServer {
    pub async fn stop(self) {
        self.shared.closed.store(true, Ordering::Relaxed);
        let _ = self.stop.send(true);
        let _ = self.task.await;
    }
}

fn bind_error(port: u16, source: &std::io::Error) -> AppError {
    if source.kind() == std::io::ErrorKind::AddrInUse {
        AppError::invalid_input(format!(
            "Port {port} is already in use. Choose another port, or 0 for any free port."
        ))
    } else {
        AppError::internal("Could not start the mock server.").with_detail(source.to_string())
    }
}

pub async fn start(shared: Arc<Shared>) -> Result<RunningServer, AppError> {
    let mock = &shared.compiled.mock;
    let host = match mock.bind {
        MockBind::Loopback => Ipv4Addr::LOCALHOST,
        MockBind::AllInterfaces => Ipv4Addr::UNSPECIFIED,
    };
    let socket = bind_exclusive(SocketAddr::from((host, mock.port)))
        .await
        .map_err(|source| bind_error(mock.port, &source))?;
    let port = socket.local_addr().map(|a| a.port()).map_err(|source| {
        AppError::internal("Could not start the mock server.").with_detail(source.to_string())
    })?;
    refuse_own_port(&shared.compiled.mock.proxy, port)?;
    refuse_exposed_internal_proxy(mock)?;
    let acceptor = if shared.compiled.mock.tls {
        Some(tls::acceptor()?)
    } else {
        None
    };
    let (stop, stopped) = watch::channel(false);
    let task = tokio::spawn(accept_loop(socket, Arc::clone(&shared), acceptor, stopped));
    Ok(RunningServer {
        port,
        shared,
        stop,
        task,
    })
}

/// A proxy target that points back at this mock would forward every request to itself.
pub(crate) fn refuse_own_port(target: &str, port: u16) -> Result<(), AppError> {
    let Ok(url) = url::Url::parse(target) else {
        return Ok(());
    };
    let local = url.host_str().is_some_and(|host| {
        let host = host.trim_start_matches('[').trim_end_matches(']');
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .is_ok_and(|ip| ip.is_loopback() || ip.is_unspecified())
    });
    if local && url.port_or_known_default() == Some(port) {
        return Err(AppError::invalid_input(
            "The proxy target is this mock itself. Point it at another server.",
        ));
    }
    Ok(())
}

/// Open connections per client address.
#[derive(Clone, Default)]
struct Clients(Arc<Mutex<HashMap<IpAddr, usize>>>);

struct ClientSlot {
    clients: Clients,
    ip: IpAddr,
}

impl Clients {
    fn admit(&self, ip: IpAddr) -> Option<ClientSlot> {
        let mut open = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let count = open.entry(ip).or_insert(0);
        if *count >= MAX_PER_CLIENT {
            return None;
        }
        *count += 1;
        Some(ClientSlot {
            clients: self.clone(),
            ip,
        })
    }
}

impl Drop for ClientSlot {
    fn drop(&mut self) {
        let mut open = self
            .clients
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = open.get_mut(&self.ip) {
            *count -= 1;
            if *count == 0 {
                open.remove(&self.ip);
            }
        }
    }
}

/// Holds a connection's slots (global permit, per-client count) for as long as the connection
/// or the WebSocket session it turned into is alive.
struct ConnGuard {
    _slot: ClientSlot,
    _permit: OwnedSemaphorePermit,
}

/// A host on this machine or a private network: never a good proxy target for a mock that
/// other computers can reach.
pub(crate) fn is_internal_host(host: &str) -> bool {
    let host = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || !host.contains('.') && host.parse::<IpAddr>().is_err()
    {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => {
            ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
        }
        Ok(IpAddr::V6(ip)) => {
            let first = ip.segments()[0];
            ip.is_loopback()
                || ip.is_unspecified()
                || first & 0xfe00 == 0xfc00
                || first & 0xffc0 == 0xfe80
                || ip
                    .to_ipv4_mapped()
                    .is_some_and(|v4| v4.is_loopback() || v4.is_private() || v4.is_link_local())
        }
        Err(_) => false,
    }
}

pub(crate) fn refuse_exposed_internal_proxy(mock: &MockEnvironment) -> Result<(), AppError> {
    if mock.bind != MockBind::AllInterfaces {
        return Ok(());
    }
    let callbacks = mock
        .routes
        .iter()
        .flat_map(|route| &route.responses)
        .chain(mock.fallback.as_ref())
        .flat_map(|response| &response.callbacks)
        .map(|callback| callback.url.as_str());
    let targets = std::iter::once(mock.proxy.as_str())
        .filter(|target| !target.is_empty())
        .chain(callbacks);
    for target in targets {
        let internal = url::Url::parse(target)
            .ok()
            .and_then(|url| url.host_str().map(is_internal_host))
            .unwrap_or(true);
        if internal {
            return Err(AppError::invalid_input(
                "A mock that listens on all interfaces cannot proxy or send callbacks to this computer or a private network. Listen on this computer only, or use a public server.",
            ));
        }
    }
    Ok(())
}

/// A name a browser on this computer uses for the mock; anything else is a rebound domain.
fn is_loopback_host(host_header: &str) -> bool {
    let host = if let Some(rest) = host_header.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        host_header.split(':').next().unwrap_or("")
    };
    host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "::1"
}

async fn accept_loop(
    socket: TcpListener,
    shared: Arc<Shared>,
    acceptor: Option<tokio_rustls::TlsAcceptor>,
    mut stopped: watch::Receiver<bool>,
) {
    let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let clients = Clients::default();
    let mut connections: JoinSet<()> = JoinSet::new();
    loop {
        let permit = tokio::select! {
            _ = stopped.changed() => break,
            Some(done) = connections.join_next(), if !connections.is_empty() => {
                drop(done);
                continue;
            }
            permit = Arc::clone(&permits).acquire_owned() => match permit {
                Ok(permit) => permit,
                Err(_) => break,
            },
        };
        let accepted = tokio::select! {
            _ = stopped.changed() => break,
            accepted = socket.accept() => accepted,
        };
        match accepted {
            Ok((stream, remote)) => {
                let Some(slot) = clients.admit(remote.ip()) else {
                    drop(stream);
                    continue;
                };
                let (shared, stopped) = (Arc::clone(&shared), stopped.clone());
                let acceptor = acceptor.clone();
                let guard = Arc::new(ConnGuard {
                    _slot: slot,
                    _permit: permit,
                });
                connections.spawn(async move {
                    let held = Arc::clone(&guard);
                    let work = async {
                        match acceptor {
                            Some(acceptor) => {
                                let handshake =
                                    tokio::time::timeout(HEADER_TIMEOUT, acceptor.accept(stream));
                                if let Ok(Ok(secure)) = handshake.await {
                                    serve(secure, remote, shared, stopped, held).await;
                                }
                            }
                            None => serve(stream, remote, shared, stopped, held).await,
                        }
                    };
                    let _ = tokio::time::timeout(MAX_CONNECTION_LIFETIME, work).await;
                    drop(guard);
                });
            }
            Err(source) => {
                tracing::warn!(mock = %shared.file, %source, "accept failed");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
    drop(socket);
    let _ = tokio::time::timeout(DRAIN_TIMEOUT, async {
        while connections.join_next().await.is_some() {}
    })
    .await;
    connections.abort_all();
}

async fn serve<S>(
    stream: S,
    remote: SocketAddr,
    shared: Arc<Shared>,
    mut stopped: watch::Receiver<bool>,
    guard: Arc<ConnGuard>,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let upgrades = Upgrades {
        stopped: stopped.clone(),
        guard,
    };
    let service =
        service_fn(move |request| handle(request, remote, Arc::clone(&shared), upgrades.clone()));
    let connection = http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(HEADER_TIMEOUT)
        .max_headers(MAX_HEADERS)
        .max_buf_size(MAX_HEAD_BYTES)
        .keep_alive(true)
        .serve_connection(TokioIo::new(stream), service)
        .with_upgrades();
    tokio::pin!(connection);
    tokio::select! {
        _ = connection.as_mut() => {}
        _ = stopped.changed() => {
            connection.as_mut().graceful_shutdown();
            let _ = connection.as_mut().await;
        }
    }
}

/// `text` cut to at most `max` bytes at a character boundary.
fn truncate_utf8(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

pub(super) fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

fn plain(status: u16, text: &str) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from(text.to_string())));
    *response.status_mut() =
        http::StatusCode::from_u16(status).unwrap_or(http::StatusCode::BAD_REQUEST);
    response
}

fn context_of(parts: &http::request::Parts, remote: SocketAddr, body: &[u8]) -> RequestCtx {
    let headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .filter_map(|(name, value)| {
            Some((name.as_str().to_string(), value.to_str().ok()?.to_string()))
        })
        .collect();
    let find = |name: &str| {
        headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
    };
    let content_type = find("content-type")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let body_text = String::from_utf8_lossy(body).into_owned();
    let body_json =
        if content_type.contains("json") || body_text.trim_start().starts_with(['{', '[']) {
            serde_json::from_str(&body_text).ok()
        } else {
            None
        };
    let form = if content_type.contains("x-www-form-urlencoded") {
        parse_pairs(&body_text)
    } else {
        Vec::new()
    };
    RequestCtx {
        method: parts.method.as_str().to_string(),
        path: parts.uri.path().to_string(),
        query: parse_pairs(parts.uri.query().unwrap_or("")),
        cookies: parse_cookies(&find("cookie").unwrap_or_default()),
        host: find("host").unwrap_or_default(),
        headers,
        body_text,
        body_json,
        form,
        params: Vec::new(),
        ip: remote.ip().to_string(),
        request_number: 0,
        data: serde_json::Value::Null,
    }
}

/// What goes on the wire: the reply with its body resolved (text, file or upstream answer).
struct Outgoing {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

async fn send_callback(client: &reqwest::Client, callback: &PreparedCallback, mock: &str) {
    let Ok(method) = reqwest::Method::from_bytes(callback.method.to_ascii_uppercase().as_bytes())
    else {
        return;
    };
    let mut request = client.request(method, &callback.url);
    for (name, value) in &callback.headers {
        request = request.header(name, value);
    }
    match request.body(callback.body.clone()).send().await {
        Ok(response) => {
            tracing::debug!(
                mock,
                status = response.status().as_u16(),
                "mock callback sent"
            );
        }
        Err(source) => tracing::warn!(mock, %source, "mock callback failed"),
    }
}

fn problem(status: u16, message: &str) -> Outgoing {
    Outgoing {
        status,
        headers: vec![("Content-Type".into(), "application/json".into())],
        body: serde_json::json!({ "error": message })
            .to_string()
            .into_bytes(),
    }
}

async fn resolve(
    shared: &Arc<Shared>,
    reply: &Reply,
    ctx: &RequestCtx,
    query: &str,
    body: Vec<u8>,
) -> Outgoing {
    if reply.forward {
        if ctx.header(proxy::HOPS_HEADER).is_some() {
            return problem(508, "Proxy loop detected.");
        }
        let Some(client) = &shared.upstream else {
            return problem(502, "Proxy mode is not available.");
        };
        return match proxy::forward(client, &shared.compiled.mock.proxy, ctx, query, body).await {
            Ok(done) => Outgoing {
                status: done.status,
                headers: done.headers,
                body: done.body,
            },
            Err(message) => problem(502, &message),
        };
    }
    if let Some(relative) = &reply.file {
        let (root, relative) = (shared.root.clone(), relative.clone());
        let read = tokio::task::spawn_blocking(move || files::read(&root, &relative)).await;
        return match read {
            Ok(Ok(bytes)) => Outgoing {
                status: reply.status,
                headers: reply.headers.clone(),
                body: bytes,
            },
            Ok(Err(message)) => problem(500, &message),
            Err(_) => problem(500, "The file could not be read."),
        };
    }
    Outgoing {
        status: reply.status,
        headers: reply.headers.clone(),
        body: reply.body.clone().into_bytes(),
    }
}

fn to_response(out: Outgoing) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from(out.body)));
    *response.status_mut() = http::StatusCode::from_u16(out.status).unwrap_or(http::StatusCode::OK);
    for (name, value) in &out.headers {
        let parsed = (
            http::HeaderName::from_bytes(name.as_bytes()),
            http::HeaderValue::from_str(value),
        );
        if let (Ok(name), Ok(value)) = parsed {
            response.headers_mut().append(name, value);
        }
    }
    response
}

/// What a WebSocket session keeps from its connection: the stop signal and the slots.
#[derive(Clone)]
struct Upgrades {
    stopped: watch::Receiver<bool>,
    guard: Arc<ConnGuard>,
}

fn header_has(parts: &http::request::Parts, name: &str, word: &str) -> bool {
    parts
        .headers
        .get_all(name)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .any(|token| token.trim().eq_ignore_ascii_case(word))
}

fn wants_websocket(parts: &http::request::Parts) -> bool {
    parts.method == http::Method::GET
        && header_has(parts, "upgrade", "websocket")
        && header_has(parts, "connection", "upgrade")
}

/// Without CORS a mock answers only pages of its own origin; WebSockets have no CORS, so the
/// `Origin` header is checked here.
fn origin_refused(mock: &MockEnvironment, ctx: &RequestCtx) -> bool {
    let Some(origin) = ctx.header("origin") else {
        return false;
    };
    let authority = origin.split_once("://").map_or(origin, |(_, rest)| rest);
    !mock.cors && !authority.eq_ignore_ascii_case(&ctx.host)
}

/// Completes the WebSocket handshake for a matched route and hands the connection to a session.
fn accept_websocket(
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

async fn handle(
    mut request: Request<Incoming>,
    remote: SocketAddr,
    shared: Arc<Shared>,
    upgrades: Upgrades,
) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let started = Instant::now();
    let upgrade = hyper::upgrade::on(&mut request);
    let (parts, body) = request.into_parts();
    let bytes = match collect_body(body, MAX_REQUEST_BODY, BODY_TIMEOUT).await {
        BodyRead::Complete(bytes) => bytes,
        BodyRead::TooLarge { .. } => return Ok(plain(413, "Payload too large.")),
        BodyRead::TimedOut => return Ok(plain(408, "Request timed out.")),
        BodyRead::Failed => return Ok(plain(400, "Bad request.")),
    };
    let ctx = context_of(&parts, remote, &bytes);
    if shared.compiled.mock.bind == MockBind::Loopback
        && !is_loopback_host(&ctx.host)
        && !ctx.host.is_empty()
    {
        return Ok(plain(421, "Misdirected request."));
    }
    if wants_websocket(&parts) {
        if let Some(plan) = ws::find(&shared.compiled, &ctx) {
            if origin_refused(&shared.compiled.mock, &ctx) {
                return Ok(plain(403, "Cross-origin WebSocket refused."));
            }
            return Ok(accept_websocket(
                plan, &parts, upgrade, &shared, remote, upgrades,
            ));
        }
    }
    let logged = (
        ctx.method.clone(),
        ctx.path.clone(),
        parts.uri.query().unwrap_or("").to_string(),
    );
    let headers: Vec<KeyValue> = ctx
        .headers
        .iter()
        .map(|(name, value)| KeyValue {
            name: name.clone(),
            value: value.clone(),
            enabled: true,
        })
        .collect();
    let body_text = truncate_utf8(&ctx.body_text, MAX_LOGGED_BODY).to_string();
    let body_truncated = body_text.len() < ctx.body_text.len();

    // Templates and regex rules run off the async workers.
    let (engine, request) = (Arc::clone(&shared), ctx.clone());
    let reply = match tokio::task::spawn_blocking(move || respond(&engine.compiled, request)).await
    {
        Ok(reply) => reply,
        Err(source) => {
            tracing::warn!(mock = %shared.file, %source, "mock engine failed");
            failed_reply()
        }
    };
    if !reply.delay.is_zero() {
        tokio::time::sleep(reply.delay).await;
    }
    let outgoing = resolve(&shared, &reply, &ctx, &logged.2, bytes.to_vec()).await;
    let duration_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
    let status = outgoing.status;
    shared.record(MockLogEntry {
        id: 0,
        at_ms: now_ms(),
        method: logged.0,
        path: logged.1,
        query: logged.2,
        headers,
        body_text,
        body_truncated,
        remote: remote.to_string(),
        route_id: reply.route_id.clone(),
        response_id: reply.response_id.clone(),
        status,
        duration_ms,
    });
    shared.send_callbacks(reply.callbacks);
    Ok(to_response(outgoing))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_never_splits_a_character() {
        assert_eq!(truncate_utf8("héllo", 2), "h");
        assert_eq!(truncate_utf8("héllo", 3), "hé");
        assert_eq!(truncate_utf8("abc", 10), "abc");
    }

    #[test]
    fn internal_hosts_are_recognised() {
        for host in [
            "localhost",
            "a.local",
            "10.1.2.3",
            "192.168.0.9",
            "172.16.0.1",
            "169.254.169.254",
            "[::1]",
            "fd00::1",
            "fe80::1",
            "intranet",
            "::ffff:127.0.0.1",
        ] {
            assert!(is_internal_host(host), "{host}");
        }
        for host in ["api.example.com", "8.8.8.8", "2606:4700::1111"] {
            assert!(!is_internal_host(host), "{host}");
        }
    }

    #[test]
    fn only_local_names_pass_the_host_check() {
        for host in ["localhost", "localhost:3000", "127.0.0.1:80", "[::1]:80"] {
            assert!(is_loopback_host(host), "{host}");
        }
        for host in ["evil.example", "127.0.0.1.evil.example", "192.168.1.5:80"] {
            assert!(!is_loopback_host(host), "{host}");
        }
    }
}
