//! HTTPS servers that require a client certificate (plan 03b §10, `spawn_mtls`): a test
//! CA made with `rcgen` signs the server certificate (for `127.0.0.1`) and the client
//! certificates it accepts. No key material is kept in the repository.

use std::sync::Arc;

use axum_server::tls_rustls::RustlsConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};

use super::server::{serve_tls, TestServer};

pub struct TestCa {
    issuer: rcgen::Issuer<'static, rcgen::KeyPair>,
    der: CertificateDer<'static>,
}

/// A client certificate and its key, PEM.
pub struct ClientPem {
    pub cert: String,
    pub key: String,
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

impl TestCa {
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

    /// A client certificate signed by this CA.
    pub fn client(&self, name: &str) -> ClientPem {
        let key = rcgen::KeyPair::generate().unwrap();
        ClientPem {
            cert: self.issue(name, &key).pem(),
            key: key.serialize_pem(),
        }
    }

    /// The usual test router over HTTPS; the handshake requires a client certificate
    /// signed by this CA.
    pub fn spawn_mtls(&self) -> TestServer {
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = self.issue("127.0.0.1", &key);
        let mut roots = RootCertStore::empty();
        roots.add(self.der.clone()).unwrap();
        let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider())
            .build()
            .unwrap();
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
        let mut config = ServerConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_client_cert_verifier(verifier)
            .with_single_cert(vec![cert.der().clone()], key)
            .unwrap();
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        serve_tls(RustlsConfig::from_config(Arc::new(config)))
    }
}
