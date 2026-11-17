//! `HttpEngine::fetch_document` (plan 06 §5): one GET for a helper that is not a send
//! (WSDL imports). Same clients, proxy, TLS and cookie jar as a send; redirects are
//! followed here so that every hop is checked; nothing is kept and nothing goes to history.

use std::sync::Arc;
use std::time::Duration;

use url::Url;

use super::super::build::{build, loggable_url};
use super::super::client::{jar_key, ClientSelector, Clients};
use super::super::cookie_jars::CookieJars;
use super::super::network::{NetConfig, Network};
use super::super::oauth::flow::token_for_send;
use super::super::oauth::OAuth;
use super::super::resolve::Resolved;
use super::super::responses::validate_id;
use super::super::send::{execute, Exchange};
use super::super::send_guard::{redact_error, with_secret};
use super::super::types::ResponseMeta;
use super::HttpEngine;
use crate::error::{AppError, ErrorCode};
use crate::store::types::{Auth, HttpVersion, RequestContent, RequestLocation};

pub const DEFAULT_MAX_DOCUMENT_BYTES: u64 = 10 * 1024 * 1024;
pub const DEFAULT_MAX_REDIRECTS: usize = 5;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const REDIRECT_STATUSES: [u16; 5] = [301, 302, 303, 307, 308];

/// Called with every redirect target before it is fetched; an error stops the fetch.
pub type HopCheck = Arc<dyn Fn(&Url) -> Result<(), AppError> + Send + Sync>;

/// How one document is fetched.
#[derive(Clone)]
pub struct FetchLimits {
    /// A larger body fails the fetch.
    pub max_bytes: u64,
    pub max_redirects: usize,
    pub timeout: Duration,
    /// `true`: `auth` and the location's cookie jar are used, on hops of the same origin as
    /// the first URL only. `false`: neither is ever sent.
    pub credentials: bool,
    pub check_hop: Option<HopCheck>,
}

impl Default for FetchLimits {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_DOCUMENT_BYTES,
            max_redirects: DEFAULT_MAX_REDIRECTS,
            timeout: DEFAULT_TIMEOUT,
            credentials: false,
            check_hop: None,
        }
    }
}

impl std::fmt::Debug for FetchLimits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FetchLimits")
            .field("max_bytes", &self.max_bytes)
            .field("max_redirects", &self.max_redirects)
            .field("credentials", &self.credentials)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedDocument {
    pub final_url: Url,
    pub content_type: Option<String>,
    pub bytes: Vec<u8>,
}

fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

fn header<'a>(meta: &'a ResponseMeta, name: &str) -> Option<&'a str> {
    meta.headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str())
}

/// The next URL when `meta` is a redirect; refused when it leaves http(s).
fn redirect_target(current: &Url, meta: &ResponseMeta) -> Result<Option<Url>, AppError> {
    if !REDIRECT_STATUSES.contains(&meta.status) {
        return Ok(None);
    }
    let Some(target) = header(meta, "location").and_then(|text| current.join(text).ok()) else {
        return Ok(None);
    };
    if !["http", "https"].contains(&target.scheme()) {
        return Err(AppError::invalid_input(
            "The server redirected to a location that is not http or https.",
        )
        .with_detail(loggable_url(current)));
    }
    Ok(Some(target))
}

impl HttpEngine {
    /// One GET of `url` under `request_id` (cancellable with [`HttpEngine::cancel`]).
    pub async fn fetch_document(
        &self,
        request_id: &str,
        location: &RequestLocation,
        url: &Url,
        auth: &Auth,
        limits: FetchLimits,
    ) -> Result<FetchedDocument, AppError> {
        let id = validate_id(request_id)?;
        let job = FetchJob {
            engine: Handles {
                version: self.version.clone(),
                clients: Arc::clone(&self.clients),
                cookies: Arc::clone(&self.cookies),
                network: Arc::clone(&self.network),
                oauth: Arc::clone(&self.oauth),
            },
            config: self.network_config().await?,
            location: location.clone(),
            body_file: self.responses.path(&id)?,
            url: url.clone(),
            auth: auth.clone(),
            limits,
            request_id: id.clone(),
        };
        let fetched = self.inflight.run(&id, job.run()).await;
        let _ = std::fs::remove_file(self.responses.path(&id)?);
        fetched
    }
}

/// The engine parts one fetch needs, owned so it runs as its own task.
struct Handles {
    version: String,
    clients: Arc<Clients>,
    cookies: Arc<CookieJars>,
    network: Arc<Network>,
    oauth: Arc<OAuth>,
}

impl HttpEngine {
    /// A send for a helper (GraphQL introspection): the full pipeline of
    /// [`HttpEngine::send_resolved`] without keeping the response. The body is returned
    /// when it is at most `max_bytes` (`None` when larger) and its file is deleted.
    pub async fn send_for_body(
        &self,
        request_id: &str,
        location: &RequestLocation,
        resolved: &Resolved,
        max_bytes: usize,
    ) -> Result<(ResponseMeta, Option<Vec<u8>>), AppError> {
        let sent = self.send_unkept(request_id, location, resolved).await;
        let path = self.unkept_body(request_id)?;
        let result = sent.and_then(|meta| {
            let fits =
                !meta.truncated && usize::try_from(meta.size_bytes).is_ok_and(|n| n <= max_bytes);
            if !fits {
                return Ok((meta, None));
            }
            let bytes = std::fs::read(&path).map_err(AppError::from)?;
            Ok((meta, (bytes.len() <= max_bytes).then_some(bytes)))
        });
        let _ = std::fs::remove_file(&path);
        result
    }
}

struct FetchJob {
    engine: Handles,
    config: Arc<NetConfig>,
    location: RequestLocation,
    body_file: std::path::PathBuf,
    url: Url,
    auth: Auth,
    limits: FetchLimits,
    request_id: String,
}

impl FetchJob {
    async fn run(self) -> Result<FetchedDocument, AppError> {
        let mut current = self.url.clone();
        for _ in 0..=self.limits.max_redirects {
            let meta = self.hop(&current).await?;
            match redirect_target(&current, &meta)? {
                None => return self.finish(current, meta),
                Some(next) => {
                    if let Some(check) = &self.limits.check_hop {
                        check(&next)?;
                    }
                    current = next;
                }
            }
        }
        Err(
            AppError::new(ErrorCode::Network, "The server redirected too many times.")
                .with_detail(loggable_url(&current)),
        )
    }

    fn finish(&self, url: Url, meta: ResponseMeta) -> Result<FetchedDocument, AppError> {
        if meta.truncated {
            return Err(AppError::invalid_input(format!(
                "The document at {} is larger than {} bytes.",
                loggable_url(&url),
                self.limits.max_bytes
            )));
        }
        if !(200..300).contains(&meta.status) {
            return Err(AppError::new(
                ErrorCode::Network,
                format!(
                    "The server answered {} for {}.",
                    meta.status,
                    loggable_url(&url)
                ),
            ));
        }
        let bytes = std::fs::read(&self.body_file).map_err(|source| {
            AppError::internal("Could not read the fetched document.")
                .with_detail(source.to_string())
        })?;
        Ok(FetchedDocument {
            final_url: url,
            content_type: meta.content_type,
            bytes,
        })
    }

    /// One request without redirects; credentials only on the first URL's origin.
    async fn hop(&self, url: &Url) -> Result<ResponseMeta, AppError> {
        let trusted = self.limits.credentials && same_origin(&self.url, url);
        let content = RequestContent {
            auth: if trusted {
                self.auth.clone()
            } else {
                Auth::None
            },
            ..RequestContent::new("GET", url.as_str())
        };
        let resolved = Resolved::unchanged(content);
        let resolved = token_for_send(&self.engine.oauth, &self.config, &self.location, &resolved)
            .await
            .map_err(|error| redact_error(error, &resolved))?;
        let resolved = with_secret(&resolved, self.config.password()).into_owned();
        let jar = trusted.then(|| jar_key(&self.location));
        if let Some(jar) = &jar {
            self.engine.cookies.open(jar).await?;
        }
        let prepared = build(&resolved.content, &self.engine.version)?;
        let exchange = Exchange {
            request_id: self.request_id.clone(),
            selector: ClientSelector::new(
                Arc::clone(&self.engine.clients),
                self.engine.network.vault(),
                Arc::clone(&self.config),
                jar.as_deref(),
                true,
                HttpVersion::Auto,
            ),
            prepared,
            timeout: Some(self.limits.timeout),
            follow_redirects: false,
            verify_tls: true,
            http_version: HttpVersion::Auto,
            body_file: self.body_file.clone(),
            max_body_bytes: self.limits.max_bytes,
        };
        let meta = execute(exchange)
            .await
            .map_err(|error| redact_error(error, &resolved));
        if let Some(jar) = &jar {
            self.engine.cookies.changed(jar);
        }
        meta
    }
}
