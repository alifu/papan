//! The byte stream under a WebSocket (plan 07 §5, §13 D2): TCP, optionally through an
//! HTTP proxy's `CONNECT` tunnel, then rustls for `wss://` with the OS trust store,
//! `verify_tls: false` as an accept-any verifier, and an M3b client certificate.

use std::net::IpAddr;
use std::sync::Arc;

use base64::Engine as _;
use hyper_util::client::proxy::matcher::Matcher;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use rustls_platform_verifier::BuilderVerifierExt;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use url::{Host, Url};
use zeroize::Zeroizing;

use crate::engine::http::types::{ProxyMode, ProxySettings};
use crate::error::{AppError, ErrorCode};

const MAX_CONNECT_HEAD_BYTES: usize = 16 * 1024;
const HTTPS_PROXY_MESSAGE: &str =
    "An https:// proxy is not supported for WebSocket. Use an http:// proxy.";

pub trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}
pub type BoxedIo = Box<dyn Io>;

/// The proxy a connection goes through: always a `CONNECT` tunnel to the target.
#[derive(Clone, PartialEq, Eq)]
pub struct ProxyHop {
    pub host: String,
    pub port: u16,
    /// The `Proxy-Authorization` value.
    pub authorization: Option<String>,
}

impl std::fmt::Debug for ProxyHop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyHop")
            .field("host", &self.host)
            .field("port", &self.port)
            .field(
                "authorization",
                &self.authorization.as_ref().map(|_| REDACTED),
            )
            .finish()
    }
}

const REDACTED: &str = "<redacted>";

/// A client certificate and key as one PEM bundle (M3b's vault value).
#[derive(Clone)]
pub struct ClientIdentity(pub Zeroizing<String>);

impl std::fmt::Debug for ClientIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ClientIdentity(<redacted>)")
    }
}

#[derive(Debug, Clone)]
pub struct TlsOptions {
    pub verify: bool,
    pub identity: Option<ClientIdentity>,
}

fn network_error(message: &str, detail: impl ToString) -> AppError {
    AppError::new(ErrorCode::Network, message).with_detail(detail.to_string())
}

fn basic(username: &str, password: &str) -> String {
    let pair = format!("{username}:{password}");
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(pair)
    )
}

fn no_proxy_list(bypass: &[String]) -> String {
    let list: Vec<&str> = bypass
        .iter()
        .map(|pattern| pattern.strip_prefix("*.").unwrap_or(pattern))
        .collect();
    list.join(",")
}

fn strip_brackets(host: &str) -> &str {
    host.trim_start_matches('[').trim_end_matches(']')
}

/// The proxy for a connection to `url` (`ws`/`wss`), decided by the matcher reqwest uses
/// for the `http`/`https` twin of the URL, so the bypass list is honoured the same way.
pub fn proxy_for(
    settings: &ProxySettings,
    password: Option<&str>,
    url: &Url,
) -> Result<Option<ProxyHop>, AppError> {
    let matcher = match settings.mode {
        ProxyMode::None => return Ok(None),
        ProxyMode::System => Matcher::from_system(),
        ProxyMode::Custom => Matcher::builder()
            .all(settings.url.clone())
            .no(no_proxy_list(&settings.bypass))
            .build(),
    };
    let mut twin = url.clone();
    let scheme = if url.scheme() == "wss" {
        "https"
    } else {
        "http"
    };
    twin.set_scheme(scheme)
        .map_err(|()| AppError::invalid_input("The URL cannot be used with a proxy."))?;
    let Ok(destination) = twin.as_str().parse::<http::Uri>() else {
        return Ok(None);
    };
    let Some(intercept) = matcher.intercept(&destination) else {
        return Ok(None);
    };
    let proxy_uri = intercept.uri();
    if proxy_uri.scheme_str() == Some("https") {
        return Err(AppError::invalid_input(HTTPS_PROXY_MESSAGE));
    }
    let host = strip_brackets(proxy_uri.host().unwrap_or_default()).to_string();
    let authorization = match settings.mode {
        ProxyMode::Custom if !settings.username.is_empty() => {
            Some(basic(&settings.username, password.unwrap_or_default()))
        }
        _ => intercept
            .basic_auth()
            .and_then(|value| value.to_str().ok().map(str::to_string)),
    };
    Ok(Some(ProxyHop {
        host,
        port: proxy_uri.port_u16().unwrap_or(80),
        authorization,
    }))
}

/// The `CONNECT` request line and headers for `authority` (`host:port`).
pub fn connect_request(authority: &str, authorization: Option<&str>) -> String {
    let auth = authorization
        .map(|value| format!("Proxy-Authorization: {value}\r\n"))
        .unwrap_or_default();
    format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n{auth}\r\n")
}

/// The status of the proxy's answer; the stream is left at the first byte of the tunnel.
async fn read_connect_status(
    stream: &mut (impl AsyncRead + Unpin),
) -> Result<(u16, String), AppError> {
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() >= MAX_CONNECT_HEAD_BYTES {
            return Err(network_error(
                "The proxy answered with an oversized reply.",
                "",
            ));
        }
        let mut byte = [0_u8; 1];
        let read = stream
            .read(&mut byte)
            .await
            .map_err(|source| network_error("The proxy closed the connection.", source))?;
        if read == 0 {
            return Err(network_error("The proxy closed the connection.", ""));
        }
        head.push(byte[0]);
    }
    let text = String::from_utf8_lossy(&head);
    let line = text.lines().next().unwrap_or_default().to_string();
    let status = line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| network_error("The proxy answered with an invalid reply.", &line))?;
    Ok((status, line))
}

async fn tunnel(proxy: &ProxyHop, authority: &str) -> Result<TcpStream, AppError> {
    let mut stream = TcpStream::connect((proxy.host.as_str(), proxy.port))
        .await
        .map_err(|source| network_error("Could not connect to the proxy.", source))?;
    let request = connect_request(authority, proxy.authorization.as_deref());
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|source| network_error("Could not talk to the proxy.", source))?;
    let (status, line) = read_connect_status(&mut stream).await?;
    if !(200..300).contains(&status) {
        let message = format!("The proxy refused the connection (status {status}).");
        return Err(network_error(&message, line));
    }
    Ok(stream)
}

/// A verifier that accepts every certificate: `verify_tls: false` (signatures are still
/// checked, so a broken handshake still fails).
#[derive(Debug)]
struct AcceptAny(Arc<CryptoProvider>);

impl ServerCertVerifier for AcceptAny {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        let algorithms = &self.0.signature_verification_algorithms;
        rustls::crypto::verify_tls12_signature(message, cert, dss, algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        let algorithms = &self.0.signature_verification_algorithms;
        rustls::crypto::verify_tls13_signature(message, cert, dss, algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

fn tls_error(source: impl ToString) -> AppError {
    network_error("Could not set up TLS.", source)
}

fn identity_parts(
    identity: &ClientIdentity,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), AppError> {
    let unusable = || AppError::invalid_input("The client certificate cannot be used.");
    let bytes = identity.0.as_bytes();
    let certificates = CertificateDer::pem_slice_iter(bytes)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| unusable())?;
    let key = PrivateKeyDer::from_pem_slice(bytes).map_err(|_| unusable())?;
    Ok((certificates, key))
}

pub fn tls_config(options: &TlsOptions) -> Result<Arc<ClientConfig>, AppError> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let builder = ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(tls_error)?;
    let wants_client = if options.verify {
        builder.with_platform_verifier().map_err(tls_error)?
    } else {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAny(provider)))
    };
    let config = match &options.identity {
        None => wants_client.with_no_client_auth(),
        Some(identity) => {
            let (certificates, key) = identity_parts(identity)?;
            wants_client
                .with_client_auth_cert(certificates, key)
                .map_err(|_| AppError::invalid_input("The client certificate cannot be used."))?
        }
    };
    Ok(Arc::new(config))
}

fn server_name(host: &Host<&str>) -> Result<ServerName<'static>, AppError> {
    match host {
        Host::Domain(name) => ServerName::try_from(name.to_string())
            .map_err(|source| network_error("The host name is not valid for TLS.", source)),
        Host::Ipv4(address) => Ok(ServerName::IpAddress(IpAddr::V4(*address).into())),
        Host::Ipv6(address) => Ok(ServerName::IpAddress(IpAddr::V6(*address).into())),
    }
}

/// Opens the stream for `url`: TCP (through `proxy` when given), then TLS for `wss`.
pub async fn connect(
    url: &Url,
    proxy: Option<&ProxyHop>,
    tls: &TlsOptions,
) -> Result<BoxedIo, AppError> {
    let host = url
        .host()
        .ok_or_else(|| AppError::invalid_input("The URL has no host."))?;
    let port = url.port_or_known_default().unwrap_or(80);
    let tls = (url.scheme() == "wss").then_some(tls);
    connect_stream(&host, port, proxy, tls, &[]).await
}

/// TCP to `host:port` (through `proxy` when given), then TLS when `tls` is set, offering
/// the `alpn` protocols (none for WebSocket, `h2` for gRPC).
pub async fn connect_stream(
    host: &Host<&str>,
    port: u16,
    proxy: Option<&ProxyHop>,
    tls: Option<&TlsOptions>,
    alpn: &[&[u8]],
) -> Result<BoxedIo, AppError> {
    let bracketed = host.to_string();
    let host_text = strip_brackets(&bracketed);
    let tcp = match proxy {
        Some(hop) => tunnel(hop, &format!("{bracketed}:{port}")).await?,
        None => TcpStream::connect((host_text, port))
            .await
            .map_err(|source| network_error("Could not connect to the server.", source))?,
    };
    // Small frames should not wait for Nagle; a failure only costs latency.
    let _ = tcp.set_nodelay(true);
    let Some(tls) = tls else {
        return Ok(Box::new(tcp));
    };
    let mut config = (*tls_config(tls)?).clone();
    config.alpn_protocols = alpn.iter().map(|protocol| protocol.to_vec()).collect();
    let connector = TlsConnector::from(Arc::new(config));
    let secured = connector
        .connect(server_name(host)?, tcp)
        .await
        .map_err(|source| {
            network_error(
                "The TLS handshake failed. Check the certificate, or turn off TLS verification in Settings.",
                source,
            )
        })?;
    Ok(Box::new(secured))
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
