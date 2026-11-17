//! A throwaway self-signed certificate for `tls: true` mocks (plan 11 §9). Made at every
//! start, kept in memory only, valid for localhost and the loopback addresses.

use std::sync::Arc;

use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio_rustls::TlsAcceptor;

use crate::error::AppError;

fn failed(source: impl std::fmt::Display) -> AppError {
    AppError::internal("Could not set up HTTPS for the mock.").with_detail(source.to_string())
}

pub fn acceptor() -> Result<TlsAcceptor, AppError> {
    let names = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ];
    let certified = rcgen::generate_simple_self_signed(names).map_err(failed)?;
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        certified.signing_key.serialize_der(),
    ));
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(failed)?
        .with_no_client_auth()
        .with_single_cert(vec![certified.cert.der().clone()], key)
        .map_err(failed)?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(TlsAcceptor::from(Arc::new(config)))
}
