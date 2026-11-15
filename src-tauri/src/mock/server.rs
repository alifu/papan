//! The HTTP/1 listener for one running mock (plan 11 §5): the same accept-loop shape as
//! the webhook listener, answering from the mock engine.

use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::net::TcpListener;
use tokio::sync::{watch, Semaphore};
use tokio::task::JoinSet;

use super::engine::{respond, Compiled, Reply};
use super::model::{MockBind, MockEnvironment};
use super::request::{parse_cookies, parse_pairs, RequestCtx};
use super::types::MockLogEntry;
use super::MockSink;
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
/// Requests larger than this get a 413 and are not matched.
pub const MAX_REQUEST_BODY: usize = 1024 * 1024;
/// Body text kept in the log per request.
const MAX_LOGGED_BODY: usize = 64 * 1024;
pub const MAX_LOG_ENTRIES: usize = 500;

/// What one running mock has served; shared with the connection tasks.
pub struct Shared {
    collection_id: String,
    file: String,
    compiled: Compiled,
    log: Mutex<VecDeque<MockLogEntry>>,
    next_id: AtomicU32,
    sink: Arc<dyn MockSink>,
}

impl Shared {
    pub fn new(
        collection_id: &str,
        file: &str,
        mock: MockEnvironment,
        sink: Arc<dyn MockSink>,
    ) -> Self {
        Self {
            collection_id: collection_id.to_string(),
            file: file.to_string(),
            compiled: Compiled::new(mock),
            log: Mutex::new(VecDeque::new()),
            next_id: AtomicU32::new(1),
            sink,
        }
    }

    pub fn log(&self) -> MutexGuard<'_, VecDeque<MockLogEntry>> {
        self.log.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn record(&self, mut entry: MockLogEntry) {
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
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

impl RunningServer {
    pub async fn stop(self) {
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
    let (stop, stopped) = watch::channel(false);
    let task = tokio::spawn(accept_loop(socket, shared, stopped));
    Ok(RunningServer { port, stop, task })
}

async fn accept_loop(socket: TcpListener, shared: Arc<Shared>, mut stopped: watch::Receiver<bool>) {
    let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
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
                let (shared, stopped) = (Arc::clone(&shared), stopped.clone());
                connections.spawn(async move {
                    serve(stream, remote, shared, stopped).await;
                    drop(permit);
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

async fn serve(
    stream: tokio::net::TcpStream,
    remote: SocketAddr,
    shared: Arc<Shared>,
    mut stopped: watch::Receiver<bool>,
) {
    let service = service_fn(move |request| handle(request, remote, Arc::clone(&shared)));
    let connection = http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(HEADER_TIMEOUT)
        .max_headers(MAX_HEADERS)
        .max_buf_size(MAX_HEAD_BYTES)
        .keep_alive(true)
        .serve_connection(TokioIo::new(stream), service);
    tokio::pin!(connection);
    tokio::select! {
        _ = connection.as_mut() => {}
        _ = stopped.changed() => {
            connection.as_mut().graceful_shutdown();
            let _ = connection.as_mut().await;
        }
    }
}

fn now_ms() -> f64 {
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
    }
}

fn to_response(reply: &Reply) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from(reply.body.clone())));
    *response.status_mut() =
        http::StatusCode::from_u16(reply.status).unwrap_or(http::StatusCode::OK);
    for (name, value) in &reply.headers {
        let parsed = (
            http::HeaderName::from_bytes(name.as_bytes()),
            http::HeaderValue::from_str(value),
        );
        if let (Ok(name), Ok(value)) = parsed {
            response.headers_mut().insert(name, value);
        }
    }
    response
}

async fn handle(
    request: Request<Incoming>,
    remote: SocketAddr,
    shared: Arc<Shared>,
) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let started = Instant::now();
    let (parts, body) = request.into_parts();
    let bytes = match collect_body(body, MAX_REQUEST_BODY, BODY_TIMEOUT).await {
        BodyRead::Complete(bytes) => bytes,
        BodyRead::TooLarge { .. } => return Ok(plain(413, "Payload too large.")),
        BodyRead::TimedOut => return Ok(plain(408, "Request timed out.")),
        BodyRead::Failed => return Ok(plain(400, "Bad request.")),
    };
    let ctx = context_of(&parts, remote, &bytes);
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
    let body_text: String = ctx.body_text.chars().take(MAX_LOGGED_BODY).collect();
    let body_truncated = body_text.len() < ctx.body_text.len();

    let reply = respond(&shared.compiled, ctx);
    if !reply.delay.is_zero() {
        tokio::time::sleep(reply.delay).await;
    }
    let duration_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
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
        status: reply.status,
        duration_ms,
    });
    Ok(to_response(&reply))
}
