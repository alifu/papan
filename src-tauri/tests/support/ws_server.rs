//! A WebSocket server for the engine tests (plan 07 §10): echoes text and binary frames,
//! records every handshake, picks a subprotocol, and acts on commands sent as text:
//! `!close:<code>:<reason>`, `!ping`, `!binary`, `!big` (16 MiB + 1 text), `!flood:<n>`.
//! `spawn_hanging` accepts connections and never answers. TLS comes from `rcgen`.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;

const BIG_MESSAGE_BYTES: usize = 16 * 1024 * 1024 + 1;

/// What the server saw of one opening handshake.
#[derive(Debug, Clone)]
pub struct Handshake {
    pub uri: String,
    pub headers: Vec<(String, String)>,
}

impl Handshake {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Default)]
struct Shared {
    handshakes: Mutex<Vec<Handshake>>,
    protocol: Option<String>,
}

pub struct WsServer {
    pub addr: SocketAddr,
    shared: Arc<Shared>,
}

impl WsServer {
    pub fn url(&self, path: &str) -> String {
        format!("ws://{}{path}", self.addr)
    }

    pub fn wss_url(&self, path: &str) -> String {
        format!("wss://{}{path}", self.addr)
    }

    pub fn handshakes(&self) -> Vec<Handshake> {
        self.shared.handshakes.lock().unwrap().clone()
    }
}

pub async fn spawn() -> WsServer {
    start(None, None).await
}

/// Selects `protocol` when the client offers it.
pub async fn spawn_with_protocol(protocol: &str) -> WsServer {
    start(Some(protocol.to_string()), None).await
}

pub async fn spawn_tls(config: Arc<ServerConfig>) -> WsServer {
    start(None, Some(TlsAcceptor::from(config))).await
}

/// Accepts TCP connections and never speaks.
pub async fn spawn_hanging() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            held.push(stream);
        }
    });
    addr
}

async fn start(protocol: Option<String>, tls: Option<TlsAcceptor>) -> WsServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let shared = Arc::new(Shared {
        protocol,
        ..Shared::default()
    });
    let state = Arc::clone(&shared);
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let state = Arc::clone(&state);
            let tls = tls.clone();
            tokio::spawn(async move {
                match tls {
                    None => serve(stream, state).await,
                    Some(acceptor) => {
                        if let Ok(secured) = acceptor.accept(stream).await {
                            serve(secured, state).await;
                        }
                    }
                }
            });
        }
    });
    WsServer { addr, shared }
}

#[allow(clippy::result_large_err)] // the handshake callback's error type is fixed by tungstenite
async fn serve<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(stream: S, state: Arc<Shared>) {
    let recorder = Arc::clone(&state);
    let callback = move |request: &Request, mut response: Response| {
        let headers = request
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.to_string(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect();
        recorder.handshakes.lock().unwrap().push(Handshake {
            uri: request.uri().to_string(),
            headers,
        });
        let offered = request
            .headers()
            .get("sec-websocket-protocol")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        if let Some(chosen) = &recorder.protocol {
            if offered.split(',').any(|item| item.trim() == chosen) {
                response
                    .headers_mut()
                    .insert("sec-websocket-protocol", chosen.parse().unwrap());
            }
        }
        Ok(response)
    };
    let Ok(mut socket) = accept_hdr_async(stream, callback).await else {
        return;
    };
    while let Some(Ok(message)) = socket.next().await {
        let reply = match message {
            Message::Text(text) => act(text.as_str()),
            Message::Binary(bytes) => vec![Message::Binary(bytes)],
            Message::Close(_) => return,
            _ => continue,
        };
        for message in reply {
            if socket.send(message).await.is_err() {
                return;
            }
        }
    }
}

fn act(text: &str) -> Vec<Message> {
    if let Some(rest) = text.strip_prefix("!close:") {
        let (code, reason) = rest.split_once(':').unwrap_or((rest, ""));
        let frame = CloseFrame {
            code: CloseCode::from(code.parse::<u16>().unwrap_or(1000)),
            reason: reason.to_string().into(),
        };
        return vec![Message::Close(Some(frame))];
    }
    if let Some(count) = text.strip_prefix("!flood:") {
        let count: usize = count.parse().unwrap_or(0);
        return (0..count).map(|n| Message::text(format!("m{n}"))).collect();
    }
    match text {
        "!ping" => vec![Message::Ping(b"hi".to_vec().into())],
        "!binary" => vec![Message::Binary(vec![1, 2, 3].into())],
        "!big" => vec![Message::text("a".repeat(BIG_MESSAGE_BYTES))],
        other => vec![Message::text(other.to_string())],
    }
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

fn server_config(
    cert: &rcgen::Certificate,
    key: &rcgen::KeyPair,
    clients: Option<Arc<dyn rustls::server::danger::ClientCertVerifier>>,
) -> Arc<ServerConfig> {
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
    let builder = ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .unwrap();
    let builder = match clients {
        Some(verifier) => builder.with_client_cert_verifier(verifier),
        None => builder.with_no_client_auth(),
    };
    Arc::new(
        builder
            .with_single_cert(vec![cert.der().clone()], key)
            .unwrap(),
    )
}

/// A server certificate nobody trusts: `verify_tls: true` must refuse it.
pub fn self_signed_config() -> Arc<ServerConfig> {
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec!["127.0.0.1".to_string()])
        .unwrap()
        .self_signed(&key)
        .unwrap();
    server_config(&cert, &key, None)
}

/// A test CA that signs a server certificate and the client certificates it accepts.
pub struct Pki {
    issuer: rcgen::Issuer<'static, rcgen::KeyPair>,
    der: CertificateDer<'static>,
}

pub struct ClientPem {
    pub cert: String,
    pub key: String,
}

impl Pki {
    pub fn new() -> Self {
        let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let key = rcgen::KeyPair::generate().unwrap();
        let der = params.self_signed(&key).unwrap().der().clone();
        Self {
            issuer: rcgen::Issuer::new(params, key),
            der,
        }
    }

    fn issue(&self, name: &str, key: &rcgen::KeyPair) -> rcgen::Certificate {
        rcgen::CertificateParams::new(vec![name.to_string()])
            .unwrap()
            .signed_by(key, &self.issuer)
            .unwrap()
    }

    pub fn client(&self, name: &str) -> ClientPem {
        let key = rcgen::KeyPair::generate().unwrap();
        ClientPem {
            cert: self.issue(name, &key).pem(),
            key: key.serialize_pem(),
        }
    }

    /// Requires a client certificate signed by this CA.
    pub fn mtls_config(&self) -> Arc<ServerConfig> {
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = self.issue("127.0.0.1", &key);
        let mut roots = RootCertStore::empty();
        roots.add(self.der.clone()).unwrap();
        let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider())
            .build()
            .unwrap();
        server_config(&cert, &key, Some(verifier))
    }
}
