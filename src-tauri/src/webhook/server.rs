//! The HTTP/1 listener (plan 07 §6): one accept loop per running webhook.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use http_body_util::BodyExt;
use hyper::body::{Body, Bytes};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::net::TcpListener;
use tokio::sync::{watch, Semaphore};
use tokio::task::JoinSet;

use super::capture::{build_capture, path_matches, CaptureRing};
use super::respond::{self, Reply};
use super::types::{Capture, CaptureSummary, WebhookBind, WebhookConfig};
use super::CaptureSink;
use crate::engine::http::oauth::listener::bind_exclusive;
use crate::error::AppError;

const HEADER_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_HEADERS: usize = 100;
const MAX_HEAD_BYTES: usize = 64 * 1024;
const MAX_CONNECTIONS: usize = 32;
/// The whole body must arrive within this, so slow clients cannot hold every permit.
const BODY_TIMEOUT: Duration = Duration::from_secs(30);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

/// What a listener has seen; outlives stop and start so the list stays visible.
pub struct Shared {
    pub id: String,
    pub ring: Mutex<CaptureRing>,
    pub unmatched: AtomicU32,
    pub sink: Arc<dyn CaptureSink>,
}

impl Shared {
    pub fn new(id: &str, max_captures: u32, sink: Arc<dyn CaptureSink>) -> Self {
        Self {
            id: id.to_string(),
            ring: Mutex::new(CaptureRing::new(max_captures as usize)),
            unmatched: AtomicU32::new(0),
            sink,
        }
    }

    pub fn ring(&self) -> std::sync::MutexGuard<'_, CaptureRing> {
        self.ring.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn record(&self, capture: Capture) -> CaptureSummary {
        let summary = capture.summary.clone();
        self.ring().push(capture);
        self.sink.captured(&self.id, &summary);
        summary
    }
}

/// A listening server. `stop` ends the accept loop, closes idle connections and frees the port.
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

fn bind_error(port: u32, source: &std::io::Error) -> AppError {
    if source.kind() == std::io::ErrorKind::AddrInUse {
        AppError::invalid_input(format!(
            "Port {port} is already in use. Choose another port, or 0 for any free port."
        ))
    } else {
        AppError::internal("Could not start the webhook listener.").with_detail(source.to_string())
    }
}

pub async fn start(config: &WebhookConfig, shared: Arc<Shared>) -> Result<RunningServer, AppError> {
    let host = match config.bind {
        WebhookBind::Loopback => Ipv4Addr::LOCALHOST,
        WebhookBind::AllInterfaces => Ipv4Addr::UNSPECIFIED,
    };
    let port = u16::try_from(config.port).unwrap_or(0);
    let socket = bind_exclusive(SocketAddr::from((host, port)))
        .await
        .map_err(|source| bind_error(config.port, &source))?;
    let bound = socket.local_addr().map(|a| a.port()).map_err(|source| {
        AppError::internal("Could not start the webhook listener.").with_detail(source.to_string())
    })?;
    let (stop, stopped) = watch::channel(false);
    let config = Arc::new(config.clone());
    let task = tokio::spawn(accept_loop(socket, config, shared, stopped));
    Ok(RunningServer {
        port: bound,
        stop,
        task,
    })
}

async fn accept_loop(
    socket: TcpListener,
    config: Arc<WebhookConfig>,
    shared: Arc<Shared>,
    mut stopped: watch::Receiver<bool>,
) {
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
                let (config, shared) = (Arc::clone(&config), Arc::clone(&shared));
                let stopped = stopped.clone();
                connections.spawn(async move {
                    serve(stream, remote, config, shared, stopped).await;
                    drop(permit);
                });
            }
            Err(source) => {
                tracing::warn!(listener = %shared.id, %source, "accept failed");
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
    config: Arc<WebhookConfig>,
    shared: Arc<Shared>,
    mut stopped: watch::Receiver<bool>,
) {
    let service = service_fn(move |request| {
        handle(request, remote, Arc::clone(&config), Arc::clone(&shared))
    });
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

type Handled = Result<Reply, std::convert::Infallible>;

async fn handle(
    request: Request<hyper::body::Incoming>,
    remote: SocketAddr,
    config: Arc<WebhookConfig>,
    shared: Arc<Shared>,
) -> Handled {
    let (parts, body) = request.into_parts();
    let path = parts.uri.path().to_string();
    if !path_matches(&config.path, &path) {
        shared.unmatched.fetch_add(1, Ordering::Relaxed);
        tracing::info!(listener = %shared.id, method = %parts.method, status = 404, size = 0, "webhook request");
        return Ok(respond::plain(404, "Not found."));
    }
    let limit = config.max_body_bytes as usize;
    let declared = parts
        .headers
        .get(http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    let (collected, seen) = if declared > limit {
        (None, declared)
    } else {
        match collect_body(body, limit, BODY_TIMEOUT).await {
            BodyRead::Complete(bytes) => (Some(bytes), 0),
            BodyRead::TooLarge { seen } => (None, seen),
            BodyRead::TimedOut => {
                tracing::info!(listener = %shared.id, method = %parts.method, status = 408, size = 0, "webhook request");
                return Ok(timeout_reply());
            }
            BodyRead::Failed => return Ok(bad_request()),
        }
    };
    let capture = build_capture(
        &parts,
        &remote.to_string(),
        collected.as_deref(),
        declared.max(seen),
    );
    let size = capture.summary.size;
    shared.record(capture);
    let (reply, code) = match collected {
        Some(_) => (
            respond::configured(&config.response),
            config.response.status,
        ),
        None => (respond::plain(413, "Payload too large."), 413),
    };
    tracing::info!(listener = %shared.id, method = %parts.method, status = code, size, "webhook request");
    Ok(reply)
}

/// How reading a request body ended.
enum BodyRead {
    Complete(Bytes),
    /// More than the cap arrived; `seen` bytes were read before giving up.
    TooLarge {
        seen: usize,
    },
    TimedOut,
    Failed,
}

/// Reads `body` up to `limit` bytes, all within `timeout`.
async fn collect_body<B>(mut body: B, limit: usize, timeout: Duration) -> BodyRead
where
    B: Body<Data = Bytes> + Unpin,
{
    let read = async {
        let mut all = Vec::new();
        while let Some(frame) = body.frame().await {
            let Ok(frame) = frame else {
                return BodyRead::Failed;
            };
            let Ok(data) = frame.into_data() else {
                continue;
            };
            if all.len() + data.len() > limit {
                return BodyRead::TooLarge {
                    seen: all.len() + data.len(),
                };
            }
            all.extend_from_slice(&data);
        }
        BodyRead::Complete(Bytes::from(all))
    };
    tokio::time::timeout(timeout, read)
        .await
        .unwrap_or(BodyRead::TimedOut)
}

fn timeout_reply() -> Reply {
    let mut reply = respond::plain(408, "Request timed out.");
    reply.headers_mut().insert(
        http::header::CONNECTION,
        http::HeaderValue::from_static("close"),
    );
    reply
}

fn bad_request() -> Reply {
    let mut response = Response::new(http_body_util::Full::new(hyper::body::Bytes::new()));
    *response.status_mut() = http::StatusCode::BAD_REQUEST;
    response
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use hyper::body::{Frame, SizeHint};

    use super::*;

    /// A body that delivers `first` and then never ends.
    struct Stalled(Option<Bytes>);

    impl Body for Stalled {
        type Data = Bytes;
        type Error = Infallible;

        fn poll_frame(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
            match self.0.take() {
                Some(bytes) => Poll::Ready(Some(Ok(Frame::data(bytes)))),
                None => Poll::Pending,
            }
        }

        fn size_hint(&self) -> SizeHint {
            SizeHint::default()
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_body_that_stalls_times_out() {
        let body = Stalled(Some(Bytes::from_static(b"abc")));

        let result = collect_body(body, 1024, BODY_TIMEOUT).await;

        assert!(matches!(result, BodyRead::TimedOut));
        assert_eq!(timeout_reply().status(), 408);
    }

    #[tokio::test]
    async fn a_body_over_the_cap_reports_the_bytes_seen() {
        let body = http_body_util::Full::new(Bytes::from(vec![b'a'; 50]));

        let result = collect_body(body, 10, BODY_TIMEOUT).await;

        assert!(matches!(result, BodyRead::TooLarge { seen: 50 }));
    }
}
