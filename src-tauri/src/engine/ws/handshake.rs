//! Everything a connection needs before the first byte is sent (plan 07 §5): the
//! resolved request becomes a [`WsConnectSpec`] through the M4 resolver's output, OAuth 2,
//! the cookie jar, the proxy and the client certificate. [`WsConnectSpec`] is plain data,
//! so M6b can build one from a GraphQL request as well.

use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{HeaderName, HeaderValue, COOKIE};
use url::Url;
use zeroize::Zeroizing;

use super::messages::Masker;
use super::transport::{proxy_for, ClientIdentity, ProxyHop, TlsOptions};
use crate::engine::http::build::{handshake_parts, HandshakeParts};
use crate::engine::http::identity::match_entry;
use crate::engine::http::network::NetConfig;
use crate::engine::http::oauth::flow::token_for_send;
use crate::engine::http::resolve::Resolved;
use crate::engine::http::types::ProxySettings;
use crate::engine::http::HttpEngine;
use crate::error::AppError;
use crate::store::types::RequestLocation;
use crate::vars::template;

const CERT_VAULT_PREFIX: &str = "n/cert/";

/// A connection to open: the URL (`ws`/`wss`), the handshake headers, and the transport.
#[derive(Clone)]
pub struct WsConnectSpec {
    pub url: Url,
    pub headers: Vec<(HeaderName, HeaderValue)>,
    pub subprotocols: Vec<String>,
    /// `None` = no connect timeout.
    pub timeout: Option<Duration>,
    pub proxy: Option<ProxyHop>,
    pub tls: TlsOptions,
    /// Secret values to mask in what is shown, logged or returned in errors.
    pub secrets: Vec<String>,
}

impl std::fmt::Debug for WsConnectSpec {
    /// Header values, secrets and the proxy credentials never appear.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|(name, _)| name.as_str()).collect();
        f.debug_struct("WsConnectSpec")
            .field("url", &masked_url(&self.url))
            .field("header_names", &names)
            .field("subprotocols", &self.subprotocols)
            .field("timeout", &self.timeout)
            .field("proxy", &self.proxy)
            .field("tls", &self.tls)
            .field("secrets", &self.secrets.len())
            .finish()
    }
}

/// `url` without `user:pass@` and with every query value replaced.
pub fn masked_url(url: &Url) -> String {
    let mut shown = url.clone();
    let _ = shown.set_username("");
    let _ = shown.set_password(None);
    if shown.query().is_some() {
        let keys: Vec<String> = url.query_pairs().map(|(key, _)| key.into_owned()).collect();
        shown
            .query_pairs_mut()
            .clear()
            .extend_pairs(keys.iter().map(|key| (key.as_str(), "***")));
    }
    shown.to_string()
}

impl WsConnectSpec {
    /// A bare spec for `url`: no headers, direct, verified TLS.
    pub fn new(url: Url) -> Self {
        Self {
            url,
            headers: Vec::new(),
            subprotocols: Vec::new(),
            timeout: None,
            proxy: None,
            tls: TlsOptions {
                verify: true,
                identity: None,
            },
            secrets: Vec::new(),
        }
    }

    pub fn masker(&self) -> Masker {
        Masker::new(self.secrets.clone())
    }
}

/// The network settings of one connection, read off the async runtime (the first read
/// may open the vault).
struct Network {
    config: Arc<NetConfig>,
    proxy: ProxySettings,
    identity: Option<(String, Zeroizing<String>)>,
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tokio::task::spawn_blocking(work).await.map_err(|source| {
        AppError::internal("Could not read the network settings.").with_detail(source.to_string())
    })?
}

/// `url` is the typed one; an unparsable URL selects no certificate (the handshake
/// reports it).
async fn network_of(http: &HttpEngine, url: Option<Url>) -> Result<Network, AppError> {
    let network = http.network();
    let twin = url.and_then(|mut url| url.set_scheme("https").ok().map(|()| url));
    blocking(move || {
        let config = network.current()?;
        let proxy = network.settings()?.proxy;
        let entry = twin
            .as_ref()
            .and_then(|twin| match_entry(&config.certificates, twin));
        let identity = match entry {
            None => None,
            Some(entry) => {
                let bundle = network
                    .vault()
                    .get(&format!("{CERT_VAULT_PREFIX}{}", entry.id))?
                    .ok_or_else(|| {
                        AppError::invalid_input(format!(
                            "The client certificate for {} is missing. Import it again.",
                            entry.host
                        ))
                    })?;
                Some((entry.host.clone(), Zeroizing::new(bundle)))
            }
        };
        Ok(Network {
            config,
            proxy,
            identity,
        })
    })
    .await
}

fn not_defined(resolved: &Resolved) -> AppError {
    let names: Vec<String> = resolved
        .unresolved
        .iter()
        .map(|name| format!("`{name}`"))
        .collect();
    AppError::invalid_input(format!(
        "Not defined: {}. Select an environment or define it.",
        names.join(", ")
    ))
}

/// A URL that still holds an unresolved variable cannot be meant as typed.
fn undefined_in_url(resolved: &Resolved) -> Option<AppError> {
    let broken = template::names(&resolved.template_url)
        .iter()
        .any(|name| resolved.unresolved.iter().any(|missing| missing == name));
    broken.then(|| not_defined(resolved))
}

async fn with_cookies(
    http: &HttpEngine,
    location: &RequestLocation,
    parts: HandshakeParts,
    verify_tls: bool,
) -> Result<HandshakeParts, AppError> {
    if parts.headers.iter().any(|(name, _)| *name == COOKIE) {
        return Ok(parts);
    }
    let sent = http
        .cookies()
        .request_header(location, &parts.url, verify_tls)
        .await?;
    let Some(value) = sent.and_then(|text| HeaderValue::from_str(&text).ok()) else {
        return Ok(parts);
    };
    let headers = parts.headers.into_iter().chain([(COOKIE, value)]).collect();
    Ok(HandshakeParts { headers, ..parts })
}

/// The spec for a resolved request: OAuth 2 turned into Bearer, cookies from the jar of
/// `location`, the proxy and client certificate its URL selects. Errors are masked.
pub async fn connect_spec(
    http: &HttpEngine,
    location: &RequestLocation,
    resolved: &Resolved,
    version: &str,
) -> Result<WsConnectSpec, AppError> {
    if let Some(error) = undefined_in_url(resolved) {
        return Err(error);
    }
    let masker = |net: Option<&NetConfig>| {
        let password = net.and_then(NetConfig::password).map(str::to_string);
        let secrets = resolved.secret_values.iter().cloned().chain(password);
        Masker::new(secrets.collect())
    };
    let typed_url = Url::parse(resolved.content.url.trim()).ok();
    let net = network_of(http, typed_url)
        .await
        .map_err(|error| mask_error(error, &masker(None)))?;
    let masker = masker(Some(&net.config));
    build_spec(http, location, resolved, version, &net)
        .await
        .map_err(|error| mask_error(error, &masker))
}

fn mask_error(error: AppError, masker: &Masker) -> AppError {
    AppError {
        message: masker.mask(&error.message),
        detail: error.detail.as_deref().map(|detail| masker.mask(detail)),
        ..error
    }
}

async fn build_spec(
    http: &HttpEngine,
    location: &RequestLocation,
    resolved: &Resolved,
    version: &str,
    net: &Network,
) -> Result<WsConnectSpec, AppError> {
    let with_token = token_for_send(http.oauth(), &net.config, location, resolved).await?;
    let content = &with_token.content;
    let verify_tls = content.settings.verify_tls;
    let parts = handshake_parts(content, version)?;
    let parts = with_cookies(http, location, parts, verify_tls).await?;
    let proxy = proxy_for(&net.proxy, net.config.password(), &parts.url)?;
    let identity = net
        .identity
        .as_ref()
        .map(|(_, bundle)| ClientIdentity(bundle.clone()));
    let timeout_ms = content.settings.timeout_ms;
    let password = net.config.password().map(str::to_string);
    Ok(WsConnectSpec {
        url: parts.url,
        headers: parts.headers,
        subprotocols: parts.subprotocols,
        timeout: (timeout_ms > 0).then(|| Duration::from_millis(timeout_ms.into())),
        proxy,
        tls: TlsOptions {
            verify: verify_tls,
            identity,
        },
        secrets: with_token
            .secret_values
            .iter()
            .cloned()
            .chain(password)
            .collect(),
    })
}

#[cfg(test)]
#[path = "handshake_tests.rs"]
mod tests;
