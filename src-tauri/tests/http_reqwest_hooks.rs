//! Spike for plan 03b §2 (O1), kept as regression tests for the reqwest hooks M3b builds on:
//! (a) `connector_layer` and a custom `Resolve` fire, (b) `Identity::from_pem` accepts
//! PKCS#8, PKCS#1 and SEC1 keys with rustls, (c) `http2_prior_knowledge` offers only `h2`.

mod support;

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use aws_lc_rs::encoding::AsDer;
use axum::extract::Request;
use axum::routing::get;
use axum::Router;
use axum_server::tls_rustls::RustlsConfig;
use base64::Engine as _;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::{Client, ClientBuilder, Identity, Version};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use support::server;

// ---- (a) resolver and connector layer -------------------------------------------------

#[derive(Default)]
struct CountingResolver {
    lookups: AtomicUsize,
}

impl Resolve for CountingResolver {
    fn resolve(&self, name: Name) -> Resolving {
        self.lookups.fetch_add(1, Ordering::SeqCst);
        let host = format!("{}:0", name.as_str());
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host(host).await?.collect();
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

#[derive(Clone)]
struct CountingLayer(Arc<AtomicUsize>);

impl<S> tower_layer::Layer<S> for CountingLayer {
    type Service = CountingConnect<S>;

    fn layer(&self, inner: S) -> Self::Service {
        CountingConnect {
            inner,
            connects: Arc::clone(&self.0),
        }
    }
}

#[derive(Clone)]
struct CountingConnect<S> {
    inner: S,
    connects: Arc<AtomicUsize>,
}

impl<S, R> tower_service::Service<R> for CountingConnect<S>
where
    S: tower_service::Service<R>,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: R) -> Self::Future {
        let connects = Arc::clone(&self.connects);
        let connecting = self.inner.call(request);
        Box::pin(async move {
            let connection = connecting.await;
            connects.fetch_add(1, Ordering::SeqCst);
            connection
        })
    }
}

#[tokio::test]
async fn custom_resolver_and_connector_layer_fire_once_per_new_connection() {
    let server = server::spawn().await;
    let resolver = Arc::new(CountingResolver::default());
    let connects = Arc::new(AtomicUsize::new(0));
    let client = Client::builder()
        .no_proxy()
        .dns_resolver(Arc::clone(&resolver))
        .connector_layer(CountingLayer(Arc::clone(&connects)))
        .build()
        .unwrap();
    let url = format!("http://localhost:{}/echo", server.addr.port());

    for _ in 0..2 {
        let response = client.get(&url).send().await.unwrap();
        assert!(response.status().is_success());
        response.bytes().await.unwrap();
    }

    assert_eq!(resolver.lookups.load(Ordering::SeqCst), 1);
    assert_eq!(
        connects.load(Ordering::SeqCst),
        1,
        "second send reuses the connection"
    );
}

// ---- TLS helpers ----------------------------------------------------------------------

fn pem(label: &str, der: &[u8]) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(der);
    let lines: Vec<&str> = encoded
        .as_bytes()
        .chunks(64)
        .map(|chunk| std::str::from_utf8(chunk).unwrap())
        .collect();
    format!(
        "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
        lines.join("\n")
    )
}

/// Reads one DER TLV at the start of `der`: (tag, contents, rest).
fn tlv(der: &[u8]) -> (u8, &[u8], &[u8]) {
    let tag = der[0];
    let (len, header) = match der[1] {
        short if short < 0x80 => (usize::from(short), 2),
        long => {
            let count = usize::from(long & 0x7f);
            let len = der[2..2 + count]
                .iter()
                .fold(0usize, |acc, byte| (acc << 8) | usize::from(*byte));
            (len, 2 + count)
        }
    };
    (tag, &der[header..header + len], &der[header + len..])
}

/// The inner key of a PKCS#8 `PrivateKeyInfo` (for RSA, a PKCS#1 `RSAPrivateKey`).
fn pkcs8_inner(pkcs8: &[u8]) -> Vec<u8> {
    let (_, info, _) = tlv(pkcs8);
    let (_, _, rest) = tlv(info); // version
    let (_, _, rest) = tlv(rest); // algorithm
    let (tag, key, _) = tlv(rest);
    assert_eq!(tag, 0x04, "privateKey is an OCTET STRING");
    key.to_vec()
}

struct ClientKeys {
    pkcs8_pem: String,
    other_pem: String,
    signer: rcgen::KeyPair,
}

fn rsa_keys() -> ClientKeys {
    let pair = aws_lc_rs::rsa::KeyPair::generate(aws_lc_rs::rsa::KeySize::Rsa2048).unwrap();
    let pkcs8 = pair.as_der().unwrap().as_ref().to_vec();
    ClientKeys {
        pkcs8_pem: pem("PRIVATE KEY", &pkcs8),
        other_pem: pem("RSA PRIVATE KEY", &pkcs8_inner(&pkcs8)),
        signer: rcgen::KeyPair::try_from(pkcs8.as_slice()).unwrap(),
    }
}

fn ec_keys() -> ClientKeys {
    let alg = &aws_lc_rs::signature::ECDSA_P256_SHA256_ASN1_SIGNING;
    let pkcs8 = aws_lc_rs::signature::EcdsaKeyPair::generate_pkcs8(
        alg,
        &aws_lc_rs::rand::SystemRandom::new(),
    )
    .unwrap()
    .as_ref()
    .to_vec();
    let pair = aws_lc_rs::signature::EcdsaKeyPair::from_pkcs8(alg, &pkcs8).unwrap();
    let sec1 = pair.private_key().as_der().unwrap().as_ref().to_vec();
    ClientKeys {
        pkcs8_pem: pem("PRIVATE KEY", &pkcs8),
        other_pem: pem("EC PRIVATE KEY", &sec1),
        signer: rcgen::KeyPair::try_from(pkcs8.as_slice()).unwrap(),
    }
}

struct Pki {
    ca: rcgen::Issuer<'static, rcgen::KeyPair>,
    ca_der: CertificateDer<'static>,
}

impl Pki {
    fn new() -> Self {
        let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let key = rcgen::KeyPair::generate().unwrap();
        let ca_der = params.self_signed(&key).unwrap().der().clone();
        Self {
            ca: rcgen::Issuer::new(params, key),
            ca_der,
        }
    }

    fn issue(&self, names: &[&str], key: &rcgen::KeyPair) -> rcgen::Certificate {
        let names: Vec<String> = names.iter().map(|name| name.to_string()).collect();
        rcgen::CertificateParams::new(names)
            .unwrap()
            .signed_by(key, &self.ca)
            .unwrap()
    }
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

/// HTTPS on `127.0.0.1:0`; replies with the HTTP version the request arrived on.
async fn spawn_tls(config: ServerConfig) -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new().route(
        "/",
        get(|request: Request| async move { format!("{:?}", request.version()) }),
    );
    let server =
        axum_server::from_tcp_rustls(listener, RustlsConfig::from_config(Arc::new(config)))
            .unwrap();
    tokio::spawn(async move { server.serve(app.into_make_service()).await.unwrap() });
    addr
}

fn server_config(pki: &Pki, client_auth: bool, alpn: &[&[u8]]) -> ServerConfig {
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = pki.issue(&["127.0.0.1"], &key);
    let builder = ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .unwrap();
    let builder = if client_auth {
        let mut roots = RootCertStore::empty();
        roots.add(pki.ca_der.clone()).unwrap();
        let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider())
            .build()
            .unwrap();
        builder.with_client_cert_verifier(verifier)
    } else {
        builder.with_no_client_auth()
    };
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
    let mut config = builder
        .with_single_cert(vec![cert.der().clone()], key_der)
        .unwrap();
    config.alpn_protocols = alpn.iter().map(|proto| proto.to_vec()).collect();
    config
}

fn tls_client() -> ClientBuilder {
    Client::builder()
        .no_proxy()
        .tls_danger_accept_invalid_certs(true)
}

async fn get_version(client: &Client, addr: SocketAddr) -> reqwest::Result<Version> {
    let response = client.get(format!("https://{addr}/")).send().await?;
    let version = response.version();
    response.error_for_status()?.bytes().await?;
    Ok(version)
}

// ---- (b) client identity key forms ----------------------------------------------------

#[tokio::test]
async fn identity_from_pem_presents_pkcs8_pkcs1_and_sec1_keys() {
    let pki = Pki::new();
    let addr = spawn_tls(server_config(&pki, true, &[b"http/1.1"])).await;
    let anonymous = tls_client().build().unwrap();
    assert!(
        get_version(&anonymous, addr).await.is_err(),
        "server requires a certificate"
    );

    for (form, keys) in [("RSA", rsa_keys()), ("EC", ec_keys())] {
        let cert_pem = pki.issue(&["client.test"], &keys.signer).pem();
        for key_pem in [&keys.pkcs8_pem, &keys.other_pem] {
            let label = key_pem.lines().next().unwrap();
            let identity = Identity::from_pem(format!("{cert_pem}{key_pem}").as_bytes())
                .unwrap_or_else(|error| panic!("{form} {label} rejected: {error}"));
            let client = tls_client().identity(identity).build().unwrap();

            let result = get_version(&client, addr).await;

            assert!(result.is_ok(), "{form} {label} not accepted: {result:?}");
        }
    }
}

// ---- (c) forced HTTP version over TLS -------------------------------------------------

#[tokio::test]
async fn http2_prior_knowledge_offers_only_h2_over_tls() {
    let pki = Pki::new();
    let h1_only = spawn_tls(server_config(&pki, false, &[b"http/1.1"])).await;
    let both = spawn_tls(server_config(&pki, false, &[b"h2", b"http/1.1"])).await;
    let auto = tls_client().build().unwrap();
    let http1 = tls_client().http1_only().build().unwrap();
    let http2 = tls_client().http2_prior_knowledge().build().unwrap();

    assert_eq!(get_version(&auto, h1_only).await.unwrap(), Version::HTTP_11);
    assert_eq!(get_version(&auto, both).await.unwrap(), Version::HTTP_2);
    assert_eq!(get_version(&http1, both).await.unwrap(), Version::HTTP_11);
    assert_eq!(get_version(&http2, both).await.unwrap(), Version::HTTP_2);
    let refused = get_version(&http2, h1_only).await.unwrap_err();
    assert!(
        refused.is_connect(),
        "no common ALPN fails the handshake: {refused:?}"
    );
}
