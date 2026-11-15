//! Everything a call needs before the first byte (plan 08 §5), mirroring M7's
//! `connect_spec`: the resolved request becomes a [`GrpcCallSpec`] through OAuth 2, the
//! proxy settings and the client certificate its URL selects. Errors are masked.

use std::time::Duration;

use http::header::HeaderMap;
use url::Url;

use super::metadata;
use crate::engine::http::oauth::flow::token_for_send;
use crate::engine::http::resolve::Resolved;
use crate::engine::http::HttpEngine;
use crate::engine::ws::handshake::{masked_url, network_of, Network};
use crate::engine::ws::messages::Masker;
use crate::engine::ws::transport::{proxy_for, ClientIdentity, ProxyHop, TlsOptions};
use crate::error::AppError;
use crate::store::types::RequestLocation;

pub const INVALID_URL_MESSAGE: &str =
    "Enter the server as http://host:port (plaintext) or https://host:port (TLS).";

/// A call to make: where, with which metadata, through which transport.
#[derive(Clone)]
pub struct GrpcCallSpec {
    /// `http` (plaintext h2c) or `https` (TLS), host and port only.
    pub url: Url,
    pub metadata: HeaderMap,
    /// `None` = the default connect timeout.
    pub timeout: Option<Duration>,
    /// `None` = no deadline.
    pub deadline: Option<Duration>,
    pub proxy: Option<ProxyHop>,
    /// `None` for `http`.
    pub tls: Option<TlsOptions>,
    pub user_agent: String,
    /// Secret values to mask in what is shown, logged or returned in errors.
    pub secrets: Vec<String>,
}

impl std::fmt::Debug for GrpcCallSpec {
    /// Metadata values, secrets and the proxy credentials never appear.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.metadata.keys().map(|name| name.as_str()).collect();
        f.debug_struct("GrpcCallSpec")
            .field("url", &masked_url(&self.url))
            .field("metadata_names", &names)
            .field("timeout", &self.timeout)
            .field("deadline", &self.deadline)
            .field("proxy", &self.proxy)
            .field("tls", &self.tls)
            .field("secrets", &self.secrets.len())
            .finish()
    }
}

impl GrpcCallSpec {
    /// A bare spec for `url`: no metadata, direct, verified TLS for `https`.
    pub fn new(url: Url) -> Self {
        let tls = (url.scheme() == "https").then_some(TlsOptions {
            verify: true,
            identity: None,
        });
        Self {
            url,
            metadata: HeaderMap::new(),
            timeout: None,
            deadline: None,
            proxy: None,
            tls,
            user_agent: "papan".to_string(),
            secrets: Vec::new(),
        }
    }

    pub fn masker(&self) -> Masker {
        Masker::new(self.secrets.clone())
    }

    /// `scheme://host:port`: what logs and the descriptor cache name the server by.
    pub fn endpoint(&self) -> String {
        format!(
            "{}://{}:{}",
            self.url.scheme(),
            self.url.host_str().unwrap_or_default(),
            self.url.port_or_known_default().unwrap_or_default()
        )
    }
}

/// `http`/`https`, a host, no credentials, no path beyond `/`, no query or fragment.
pub fn parse_url(text: &str) -> Result<Url, AppError> {
    let invalid = || AppError::invalid_input(INVALID_URL_MESSAGE);
    let url = Url::parse(text.trim()).map_err(|_| invalid())?;
    let ok = matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some_and(|host| !host.is_empty())
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(url.path(), "" | "/")
        && url.query().is_none()
        && url.fragment().is_none();
    if ok {
        Ok(url)
    } else {
        Err(invalid())
    }
}

pub fn mask_error(error: AppError, masker: &Masker) -> AppError {
    AppError {
        message: masker.mask(&error.message),
        detail: error.detail.as_deref().map(|detail| masker.mask(detail)),
        ..error
    }
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

/// The spec for a resolved request. Errors are masked.
pub async fn call_spec(
    http: &HttpEngine,
    location: &RequestLocation,
    resolved: &Resolved,
    version: &str,
) -> Result<GrpcCallSpec, AppError> {
    if !resolved.unresolved.is_empty() {
        return Err(not_defined(resolved));
    }
    let typed_url = Url::parse(resolved.content.url.trim()).ok();
    let plain = Masker::new(resolved.secret_values.clone());
    let net = network_of(http, typed_url)
        .await
        .map_err(|error| mask_error(error, &plain))?;
    let password = net.config.password().map(str::to_string);
    let secrets = resolved.secret_values.iter().cloned().chain(password);
    let masker = Masker::new(secrets.collect());
    build_spec(http, location, resolved, version, &net)
        .await
        .map_err(|error| mask_error(error, &masker))
}

async fn build_spec(
    http: &HttpEngine,
    location: &RequestLocation,
    resolved: &Resolved,
    version: &str,
    net: &Network,
) -> Result<GrpcCallSpec, AppError> {
    let with_token = token_for_send(http.oauth(), &net.config, location, resolved).await?;
    let content = &with_token.content;
    let url = parse_url(&content.url)?;
    let metadata = metadata::build(content)?;
    let is_tls = url.scheme() == "https";
    // The proxy matcher of M7 decides on the `ws`/`wss` twin of a URL.
    let mut twin = url.clone();
    twin.set_scheme(if is_tls { "wss" } else { "ws" })
        .map_err(|()| AppError::invalid_input(INVALID_URL_MESSAGE))?;
    let proxy = proxy_for(&net.proxy, net.config.password(), &twin)?;
    let identity = net
        .identity
        .as_ref()
        .map(|(_, bundle)| ClientIdentity(bundle.clone()));
    let millis = |value: u32| (value > 0).then(|| Duration::from_millis(value.into()));
    let password = net.config.password().map(str::to_string);
    Ok(GrpcCallSpec {
        url,
        metadata,
        timeout: millis(content.settings.timeout_ms),
        deadline: millis(content.grpc.deadline_ms),
        proxy,
        tls: is_tls.then_some(TlsOptions {
            verify: content.settings.verify_tls,
            identity,
        }),
        user_agent: format!("papan/{version}"),
        secrets: with_token
            .secret_values
            .iter()
            .cloned()
            .chain(password)
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use http::header::{HeaderName, HeaderValue};

    use super::*;

    #[test]
    fn only_bare_http_and_https_origins_are_accepted() {
        for good in [
            "http://localhost:50051",
            "https://api.example.com/",
            " https://[::1]:443 ",
        ] {
            assert!(parse_url(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "localhost:50051",
            "grpc://host:1",
            "ws://host:1",
            "https://host/pkg.Svc",
            "https://host?key=1",
            "https://host/#x",
            "https://user:pw@host",
            "https://",
        ] {
            let error = parse_url(bad).unwrap_err();
            assert_eq!(error.message, INVALID_URL_MESSAGE, "{bad}");
        }
    }

    #[test]
    fn debug_shows_no_metadata_value_secret_or_proxy_credential() {
        let mut spec = GrpcCallSpec::new(Url::parse("https://example.com:8443").unwrap());
        spec.metadata.insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("Bearer tok-123"),
        );
        spec.secrets = vec!["tok-123".into()];
        spec.proxy = Some(ProxyHop {
            host: "proxy".into(),
            port: 3128,
            authorization: Some("Basic cHc=".into()),
        });

        let text = format!("{spec:?}");

        assert!(text.contains("authorization"));
        assert!(!text.contains("tok-123"), "{text}");
        assert!(!text.contains("cHc="), "{text}");
        assert_eq!(spec.endpoint(), "https://example.com:8443");
        assert!(spec.tls.is_some());
        let plain = GrpcCallSpec::new(Url::parse("http://h").unwrap());
        assert!(plain.tls.is_none());
        assert_eq!(plain.endpoint(), "http://h:80");
    }

    #[test]
    fn errors_are_masked_and_undefined_variables_are_named() {
        let masker = Masker::new(vec!["hunter2".into()]);
        let error = AppError::invalid_input("bad hunter2").with_detail("more hunter2");

        let masked = mask_error(error, &masker);

        assert!(!masked.message.contains("hunter2"));
        assert!(!masked.detail.unwrap().contains("hunter2"));
        let resolved = Resolved {
            unresolved: vec!["host".into()],
            ..Resolved::default()
        };
        assert!(not_defined(&resolved).message.contains("`host`"));
    }
}
