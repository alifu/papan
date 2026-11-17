//! A gRPC test server (plan 08 §8 O1) for `test.v1.Echo` (`fixtures/grpc/server/`):
//! hand-written services over `tonic::server::Grpc` + prost (no build.rs, no protoc),
//! reflection v1 and v1alpha from a protox-compiled descriptor set, h2c or TLS (rcgen +
//! tokio-rustls, ALPN `h2`, optional required client certificate via `mtls::TestCa`),
//! an optional Bearer token on every service, and a record of each call's metadata.

pub mod client;
mod echo;
mod guard;
pub mod messages;

use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use prost_types::FileDescriptorSet;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::ServerConfig;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;
use tonic::body::Body;
use tonic::codegen::http::{self, HeaderMap};
use tonic::transport::server::Connected;
use tonic::transport::Server;

use echo::Echo;
use guard::Guarded;

const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Which reflection services the server offers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Reflection {
    #[default]
    Both,
    V1Only,
    V1AlphaOnly,
    Off,
}

#[derive(Default)]
pub struct Options {
    /// TLS when set; ALPN is forced to `h2`. `None` serves h2c (prior knowledge).
    pub tls: Option<ServerConfig>,
    /// Every call (reflection too) must carry `authorization: Bearer <token>`.
    pub token: Option<String>,
    pub reflection: Reflection,
}

/// One request as the server received it: path and every header, `-bin` values as sent.
#[derive(Debug, Clone)]
pub struct ReceivedCall {
    pub path: String,
    pub metadata: Vec<(String, String)>,
}

impl ReceivedCall {
    fn from_request(request: &http::Request<Body>) -> Self {
        let metadata = request
            .headers()
            .iter()
            .map(|(name, value)| {
                let value = String::from_utf8_lossy(value.as_bytes()).into_owned();
                (name.as_str().to_string(), value)
            })
            .collect();
        Self {
            path: request.uri().path().to_string(),
            metadata,
        }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.metadata
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// One accepted connection: the negotiated ALPN (TLS only) and whether a client
/// certificate was presented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionInfo {
    pub alpn: Option<Vec<u8>>,
    pub client_cert: bool,
}

#[derive(Default)]
struct State {
    token: Option<String>,
    calls: Mutex<Vec<ReceivedCall>>,
    connections: Mutex<Vec<ConnectionInfo>>,
    cancels: AtomicUsize,
}

impl State {
    fn record(&self, call: ReceivedCall) {
        self.calls.lock().unwrap().push(call);
    }

    fn note_cancel(&self) {
        self.cancels.fetch_add(1, Ordering::SeqCst);
    }

    fn authorized(&self, headers: &HeaderMap) -> bool {
        let Some(token) = &self.token else {
            return true;
        };
        let expected = format!("Bearer {token}");
        headers
            .get(http::header::AUTHORIZATION)
            .is_some_and(|value| value.as_bytes() == expected.as_bytes())
    }
}

pub struct GrpcServer {
    pub addr: SocketAddr,
    /// The self-signed server certificate, PEM (`spawn_tls` only).
    pub cert_pem: Option<String>,
    tls: bool,
    state: Arc<State>,
    shutdown: Option<oneshot::Sender<()>>,
    tasks: Vec<JoinHandle<()>>,
}

impl GrpcServer {
    /// `http://127.0.0.1:<port>` or `https://...` for TLS.
    pub fn url(&self) -> String {
        let scheme = if self.tls { "https" } else { "http" };
        format!("{scheme}://{}", self.addr)
    }

    pub fn calls(&self) -> Vec<ReceivedCall> {
        self.state.calls.lock().unwrap().clone()
    }

    /// The latest call whose path ends with `suffix` (e.g. `"/Unary"`).
    pub fn last_call(&self, suffix: &str) -> Option<ReceivedCall> {
        self.calls()
            .into_iter()
            .rev()
            .find(|call| call.path.ends_with(suffix))
    }

    pub fn connections(&self) -> Vec<ConnectionInfo> {
        self.state.connections.lock().unwrap().clone()
    }

    /// Calls the server saw cancelled: `Sleep` dropped early, a stream whose client left.
    pub fn cancelled(&self) -> usize {
        self.state.cancels.load(Ordering::SeqCst)
    }

    /// Waits until at least `count` cancellations were seen; false on timeout.
    pub async fn wait_for_cancels(&self, count: usize, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while self.cancelled() < count {
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
        true
    }

    /// Stops accepting and serving; the port is closed when this returns.
    pub async fn stop(mut self) {
        self.halt();
        for task in std::mem::take(&mut self.tasks) {
            let _ = task.await;
        }
    }

    fn halt(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.tasks.iter().for_each(JoinHandle::abort);
    }
}

impl Drop for GrpcServer {
    fn drop(&mut self) {
        self.halt();
    }
}

/// The directory holding `test/v1/echo.proto` and `test/v1/types.proto`.
pub fn proto_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/grpc/server")
}

/// `test/v1/echo.proto` and its imports, compiled with protox.
pub fn descriptor_set() -> FileDescriptorSet {
    protox::compile(["test/v1/echo.proto"], [proto_root()]).unwrap()
}

/// h2c, both reflection services, no token.
pub async fn spawn() -> GrpcServer {
    spawn_with(Options::default()).await
}

/// TLS with a self-signed certificate for `127.0.0.1` (`cert_pem`), ALPN `h2`.
pub async fn spawn_tls() -> GrpcServer {
    let (config, pem) = self_signed();
    let mut server = spawn_with(Options {
        tls: Some(config),
        ..Options::default()
    })
    .await;
    server.cert_pem = Some(pem);
    server
}

pub async fn spawn_with(options: Options) -> GrpcServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let tls = options.tls.is_some();
    let acceptor = options.tls.map(|mut config| {
        config.alpn_protocols = vec![b"h2".to_vec()];
        TlsAcceptor::from(Arc::new(config))
    });
    let state = Arc::new(State {
        token: options.token,
        ..State::default()
    });
    let (conn_tx, conn_rx) = mpsc::channel(16);
    let accept = tokio::spawn(accept_loop(listener, acceptor, Arc::clone(&state), conn_tx));
    let (shutdown, signal) = oneshot::channel();
    let serve = tokio::spawn(serve(state.clone(), options.reflection, conn_rx, signal));
    GrpcServer {
        addr,
        cert_pem: None,
        tls,
        state,
        shutdown: Some(shutdown),
        tasks: vec![accept, serve],
    }
}

fn self_signed() -> (ServerConfig, String) {
    let names = vec!["127.0.0.1".to_string(), "localhost".to_string()];
    let certified = rcgen::generate_simple_self_signed(names).unwrap();
    let key = PrivatePkcs8KeyDer::from(certified.signing_key.serialize_der());
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![certified.cert.der().clone()],
            PrivateKeyDer::Pkcs8(key),
        )
        .unwrap();
    (config, certified.cert.pem())
}

async fn serve(
    state: Arc<State>,
    reflection: Reflection,
    connections: mpsc::Receiver<io::Result<Conn>>,
    signal: oneshot::Receiver<()>,
) {
    let set = descriptor_set();
    let builder =
        || tonic_reflection::server::Builder::configure().register_file_descriptor_set(set.clone());
    let wants_v1 = matches!(reflection, Reflection::Both | Reflection::V1Only);
    let wants_v1alpha = matches!(reflection, Reflection::Both | Reflection::V1AlphaOnly);
    let v1 = wants_v1.then(|| guard(&state, builder().build_v1().unwrap()));
    let v1alpha = wants_v1alpha.then(|| guard(&state, builder().build_v1alpha().unwrap()));
    let echo = guard(
        &state,
        Echo {
            state: Arc::clone(&state),
        },
    );
    let incoming = futures_util::stream::unfold(connections, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    });
    let _ = Server::builder()
        .add_service(echo)
        .add_optional_service(v1)
        .add_optional_service(v1alpha)
        .serve_with_incoming_shutdown(incoming, async {
            let _ = signal.await;
        })
        .await;
}

fn guard<S>(state: &Arc<State>, inner: S) -> Guarded<S> {
    Guarded {
        inner,
        state: Arc::clone(state),
    }
}

async fn accept_loop(
    listener: TcpListener,
    acceptor: Option<TlsAcceptor>,
    state: Arc<State>,
    connections: mpsc::Sender<io::Result<Conn>>,
) {
    while let Ok((tcp, _)) = listener.accept().await {
        let _ = tcp.set_nodelay(true);
        let Some(acceptor) = acceptor.clone() else {
            state.connections.lock().unwrap().push(ConnectionInfo {
                alpn: None,
                client_cert: false,
            });
            if connections.send(Ok(Conn(Box::new(tcp)))).await.is_err() {
                return;
            }
            continue;
        };
        let (state, connections) = (Arc::clone(&state), connections.clone());
        tokio::spawn(async move {
            // A failed handshake (untrusted cert, missing client cert) just drops the socket.
            let Ok(stream) = acceptor.accept(tcp).await else {
                return;
            };
            let session = stream.get_ref().1;
            state.connections.lock().unwrap().push(ConnectionInfo {
                alpn: session.alpn_protocol().map(<[u8]>::to_vec),
                client_cert: session.peer_certificates().is_some(),
            });
            let _ = connections.send(Ok(Conn(Box::new(stream)))).await;
        });
    }
}

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// A plain or TLS connection; tonic's `Connected` for TLS needs a feature we leave off.
struct Conn(Box<dyn Io>);

impl Connected for Conn {
    type ConnectInfo = ();

    fn connect_info(&self) {}
}

impl AsyncRead for Conn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut *self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for Conn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut *self.0).poll_write(cx, buf)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut *self.0).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.0.is_write_vectored()
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut *self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut *self.0).poll_shutdown(cx)
    }
}
