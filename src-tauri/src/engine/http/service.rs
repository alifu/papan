//! `HttpEngine`: the plain-Rust API behind the HTTP commands (plan 03 §6). Managed by
//! Tauri as the engine state; tests drive it directly against a local server.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::HeaderName;

use super::build::{build, PartValue, Prepared, PreparedBody, PreparedPart, INVALID_URL_MESSAGE};
use super::client::{jar_key, ClientSelector, Clients};
use super::cookie_jars::{CookieJars, Persisted};
use super::grants::{grant_scope, Grants};
use super::inflight::Inflight;
use super::network::{NetConfig, Network};
use super::resolve::Resolved;
use super::responses::{validate_id, Responses, MAX_BODY_BYTES};
use super::send::{execute, Exchange};
use super::types::{PickedFile, ResponseBody, ResponseMeta};
use crate::error::{AppError, ErrorCode};
use crate::store::history::{Outcome, Sent};
use crate::store::request_model::validate_content;
use crate::store::types::{Auth, RequestContent, RequestLocation};
use crate::store::StoreState;
use crate::vars::template;
use crate::vars::vault::{MemoryVault, SecretStore};

const CONTENT_LABEL: &str = "Request";
const UNNAMED: &str = "Untitled";

#[derive(Debug)]
pub struct HttpEngine {
    version: String,
    clients: Arc<Clients>,
    cookies: Arc<CookieJars>,
    network: Arc<Network>,
    responses: Responses,
    inflight: Inflight,
    grants: Grants,
    max_body_bytes: u64,
}

impl HttpEngine {
    /// Response bodies go under `cache_dir`; file grants and network settings live in
    /// `app_data`. Secrets are kept in memory until [`HttpEngine::with_vault`].
    pub fn new(cache_dir: &Path, app_data: &Path, version: &str) -> Result<Self, AppError> {
        let clients = Arc::new(Clients::default());
        Ok(Self {
            version: version.to_string(),
            cookies: Arc::new(CookieJars::new(Arc::clone(&clients), None)),
            clients,
            network: Arc::new(Network::load(app_data, Arc::new(MemoryVault::default()))),
            responses: Responses::new(cache_dir)?,
            inflight: Inflight::default(),
            grants: Grants::load(app_data),
            max_body_bytes: MAX_BODY_BYTES,
        })
    }

    /// Network secrets and the cookie key are read from `vault`; jars persist under `cookies`.
    #[must_use]
    pub fn with_vault(self, app_data: &Path, vault: Arc<dyn SecretStore>) -> Self {
        let persisted = Persisted {
            dir: app_data.join("cookies"),
            vault: Arc::clone(&vault),
        };
        Self {
            cookies: Arc::new(CookieJars::new(Arc::clone(&self.clients), Some(persisted))),
            network: Arc::new(Network::load(app_data, vault)),
            ..self
        }
    }

    /// The machine-local network settings.
    pub fn network(&self) -> Arc<Network> {
        Arc::clone(&self.network)
    }

    /// The cookie jars (the cookie manager, plan 03b §6.5).
    pub fn cookies(&self) -> Arc<CookieJars> {
        Arc::clone(&self.cookies)
    }

    /// Test-only: a lower body cap.
    #[must_use]
    pub fn with_max_body_bytes(self, max_body_bytes: u64) -> Self {
        Self {
            max_body_bytes,
            ..self
        }
    }

    /// Sends `content` as typed (no variables); `location` picks the cookie jar.
    pub async fn send(
        &self,
        request_id: &str,
        location: &RequestLocation,
        content: &RequestContent,
    ) -> Result<ResponseMeta, AppError> {
        self.send_resolved(request_id, location, &Resolved::unchanged(content.clone()))
            .await
    }

    /// Sends a resolved draft. Secret values are masked in the shown URL, the log line
    /// and any error; unresolved names are reported in the response.
    pub async fn send_resolved(
        &self,
        request_id: &str,
        location: &RequestLocation,
        resolved: &Resolved,
    ) -> Result<ResponseMeta, AppError> {
        if let Some(error) = undefined_in_host(resolved) {
            return Err(error);
        }
        let config = self.network_config().await?;
        let resolved = with_secret(&with_digest_secret(resolved), config.password()).into_owned();
        let resolved = &resolved;
        match self
            .send_inner(request_id, location, resolved, config)
            .await
        {
            Ok(meta) => Ok(ResponseMeta {
                url: resolved.redact(&meta.url),
                unresolved: resolved.unresolved.clone(),
                ..meta
            }),
            Err(error) => Err(redact_error(error, resolved)),
        }
    }

    async fn send_inner(
        &self,
        request_id: &str,
        location: &RequestLocation,
        resolved: &Resolved,
        config: Arc<NetConfig>,
    ) -> Result<ResponseMeta, AppError> {
        let content = &resolved.content;
        let id = validate_id(request_id)?;
        if self.responses.is_kept(&id) {
            return Err(AppError::invalid_input(
                "That request id was already used. Send again with a new id.",
            ));
        }
        validate_content(CONTENT_LABEL, content)?;
        let built = build(content, &self.version)?;
        let prepared = self.authorize(location, with_secret_headers(built, resolved))?;
        let settings = content.settings;
        let jar = jar_key(location);
        self.cookies.open(&jar).await?;
        tracing::info!(
            method = %prepared.method,
            url = %resolved.loggable_url(),
            "sending request"
        );
        let exchange = Exchange {
            request_id: id.clone(),
            selector: ClientSelector::new(
                Arc::clone(&self.clients),
                self.network.vault(),
                config,
                Some(&jar),
                settings.verify_tls,
                settings.http_version,
            ),
            prepared,
            timeout: (settings.timeout_ms > 0)
                .then(|| Duration::from_millis(settings.timeout_ms.into())),
            follow_redirects: settings.follow_redirects,
            verify_tls: settings.verify_tls,
            http_version: settings.http_version,
            body_file: self.responses.path(&id)?,
            max_body_bytes: self.max_body_bytes,
        };
        let sent = self.inflight.run(&id, execute(exchange)).await;
        if settings.verify_tls {
            self.cookies.changed(&jar);
        }
        let meta = sent?;
        self.responses.keep(&id, meta.content_type.clone());
        Ok(meta)
    }

    /// The network settings for one send, read off the async runtime (the first read
    /// may open the vault).
    async fn network_config(&self) -> Result<Arc<NetConfig>, AppError> {
        let network = Arc::clone(&self.network);
        tokio::task::spawn_blocking(move || network.current())
            .await
            .map_err(|source| {
                AppError::internal("Could not read the network settings.")
                    .with_detail(source.to_string())
            })?
    }

    /// Every file to upload must be granted; paths are replaced by their canonical form.
    fn authorize(
        &self,
        location: &RequestLocation,
        prepared: Prepared,
    ) -> Result<Prepared, AppError> {
        let scope = grant_scope(location);
        let body = match prepared.body {
            PreparedBody::File(path) => PreparedBody::File(self.grants.check(&scope, &path)?),
            PreparedBody::Multipart(parts) => PreparedBody::Multipart(
                parts
                    .into_iter()
                    .map(|part| self.authorize_part(&scope, part))
                    .collect::<Result<_, _>>()?,
            ),
            other => other,
        };
        Ok(Prepared { body, ..prepared })
    }

    fn authorize_part(&self, scope: &str, part: PreparedPart) -> Result<PreparedPart, AppError> {
        Ok(match part.value {
            PartValue::File(path) => PreparedPart {
                value: PartValue::File(self.grants.check(scope, &path)?),
                ..part
            },
            PartValue::Text(_) => part,
        })
    }

    pub fn cancel(&self, request_id: &str) -> Result<(), AppError> {
        self.inflight.cancel(&validate_id(request_id)?);
        Ok(())
    }

    pub fn in_flight(&self) -> usize {
        self.inflight.len()
    }

    pub fn read_body(&self, request_id: &str) -> Result<ResponseBody, AppError> {
        self.responses.preview(request_id)
    }

    /// The full body file of a kept response (for "Save to file").
    pub fn body_file(&self, request_id: &str) -> Result<PathBuf, AppError> {
        self.responses.file_of(request_id)
    }

    pub fn clear_cookies(&self, location: &RequestLocation) {
        self.cookies.clear(&jar_key(location));
    }

    /// Grants a file picked in the native dialog to the request at `location`.
    pub fn grant(&self, location: &RequestLocation, picked: &Path) -> Result<PickedFile, AppError> {
        self.grants.grant(&grant_scope(location), picked)
    }

    /// Carries the upload grants of a scratch request that moved into a collection.
    pub fn carry_grants(
        &self,
        from: &RequestLocation,
        to: &RequestLocation,
    ) -> Result<(), AppError> {
        self.grants.carry(&grant_scope(from), &grant_scope(to))
    }
}

/// The Digest password is masked like a secret value (plan 03b §6.3).
fn with_digest_secret(resolved: &Resolved) -> Cow<'_, Resolved> {
    match &resolved.content.auth {
        Auth::Digest { password, .. } => with_secret(resolved, Some(password)),
        _ => Cow::Borrowed(resolved),
    }
}

/// `resolved` with `secret` added to the values masked in the URL and errors; the proxy
/// password goes through here for every send (§15.2).
fn with_secret<'a>(resolved: &'a Resolved, secret: Option<&str>) -> Cow<'a, Resolved> {
    match secret {
        Some(secret)
            if !secret.is_empty() && !resolved.secret_values.iter().any(|s| s == secret) =>
        {
            Cow::Owned(Resolved {
                secret_values: resolved
                    .secret_values
                    .iter()
                    .cloned()
                    .chain([secret.to_string()])
                    .collect(),
                ..resolved.clone()
            })
        }
        _ => Cow::Borrowed(resolved),
    }
}

fn with_secret_headers(prepared: Prepared, resolved: &Resolved) -> Prepared {
    let extra = resolved
        .secret_headers
        .iter()
        .filter_map(|name| HeaderName::from_bytes(name.as_bytes()).ok());
    Prepared {
        secret_headers: prepared.secret_headers.into_iter().chain(extra).collect(),
        ..prepared
    }
}

fn not_defined(resolved: &Resolved) -> AppError {
    let names: Vec<String> = resolved
        .unresolved
        .iter()
        .map(|n| format!("`{n}`"))
        .collect();
    AppError::invalid_input(format!(
        "Not defined: {}. Select an environment or define it.",
        names.join(", ")
    ))
}

/// An unresolved token left in the scheme or host: the URL cannot be meant as typed.
fn undefined_in_host(resolved: &Resolved) -> Option<AppError> {
    let url = resolved.content.url.trim();
    let without_scheme = ["https://", "http://"]
        .iter()
        .find_map(|scheme| {
            let head = url.get(..scheme.len())?;
            head.eq_ignore_ascii_case(scheme)
                .then(|| &url[scheme.len()..])
        })
        .unwrap_or(url);
    let authority = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let broken = template::names(authority)
        .iter()
        .any(|name| resolved.unresolved.iter().any(|missing| missing == name));
    broken.then(|| not_defined(resolved))
}

/// Masks secrets in a failed send's message and detail. An invalid URL that still has
/// unresolved variables names them instead.
fn redact_error(error: AppError, resolved: &Resolved) -> AppError {
    if error.code == ErrorCode::InvalidInput
        && error.message == INVALID_URL_MESSAGE
        && !resolved.unresolved.is_empty()
    {
        let names: Vec<String> = resolved
            .unresolved
            .iter()
            .map(|n| format!("`{n}`"))
            .collect();
        return AppError::invalid_input(format!(
            "Not defined: {}. Select an environment or define it.",
            names.join(", ")
        ));
    }
    AppError {
        message: resolved.redact(&error.message),
        detail: error
            .detail
            .as_deref()
            .map(|detail| resolved.redact(detail)),
        ..error
    }
}

/// Appends a finished send to the history (not a cancelled one). The store lock is taken
/// only after the network exchange. A failure to record is logged, never surfaced.
pub fn record_history(
    store: &StoreState,
    location: &RequestLocation,
    content: &RequestContent,
    sent_at_ms: f64,
    elapsed: Duration,
    result: &Result<ResponseMeta, AppError>,
) {
    let outcome = match result {
        Ok(meta) => Outcome::Response {
            status: meta.status,
            total_ms: meta.total_ms,
            size_bytes: meta.size_bytes,
        },
        Err(error) if error.code == ErrorCode::Cancelled => return,
        Err(error) => Outcome::Failed {
            code: error.code,
            total_ms: u32::try_from(elapsed.as_millis()).unwrap_or(u32::MAX),
        },
    };
    let mut guard = store.lock_store();
    let name = guard
        .request_name(location)
        .unwrap_or_else(|| UNNAMED.to_string());
    let sent = Sent {
        location,
        name: &name,
        content,
        sent_at_ms,
        outcome,
    };
    if let Err(error) = guard.append_history(&sent) {
        tracing::warn!(detail = ?error.detail, "could not record history: {error}");
    }
}
