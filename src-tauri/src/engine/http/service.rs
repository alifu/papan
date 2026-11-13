//! `HttpEngine`: the plain-Rust API behind the HTTP commands (plan 03 §6). Managed by
//! Tauri as the engine state; tests drive it directly against a local server.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::build::{build, PartValue, Prepared, PreparedBody, PreparedPart};
use super::client::{jar_key, ClientSelector, Clients};
use super::cookie_jars::{CookieJars, Persisted};
use super::grants::{grant_scope, Grants};
use super::inflight::Inflight;
use super::network::{NetConfig, Network};
use super::oauth::flow::token_for_send;
use super::oauth::OAuth;
use super::resolve::Resolved;
use super::responses::{validate_id, Responses, MAX_BODY_BYTES};
use super::send::{execute, Exchange};
use super::send_guard::{
    redact_error, undefined_in_host, with_digest_secret, with_secret, with_secret_headers,
};
use super::types::{PickedFile, ResponseBody, ResponseMeta};
use crate::error::{AppError, ErrorCode};
use crate::store::history::{Outcome, Sent};
use crate::store::request_model::validate_content;
use crate::store::types::{RequestContent, RequestLocation};
use crate::store::StoreState;
use crate::vars::vault::{MemoryVault, SecretStore};

const CONTENT_LABEL: &str = "Request";
const UNNAMED: &str = "Untitled";

#[derive(Debug)]
pub struct HttpEngine {
    version: String,
    clients: Arc<Clients>,
    cookies: Arc<CookieJars>,
    network: Arc<Network>,
    oauth: Arc<OAuth>,
    responses: Responses,
    inflight: Inflight,
    grants: Arc<Grants>,
    max_body_bytes: u64,
}

impl HttpEngine {
    /// Response bodies go under `cache_dir`; file grants and network settings live in
    /// `app_data`. Secrets are kept in memory until [`HttpEngine::with_vault`].
    pub fn new(cache_dir: &Path, app_data: &Path, version: &str) -> Result<Self, AppError> {
        let clients = Arc::new(Clients::default());
        let network = Arc::new(Network::load(app_data, Arc::new(MemoryVault::default())));
        Ok(Self {
            version: version.to_string(),
            cookies: Arc::new(CookieJars::new(Arc::clone(&clients), None)),
            oauth: Arc::new(OAuth::new(Arc::clone(&clients), Arc::clone(&network))),
            clients,
            network,
            responses: Responses::new(cache_dir)?,
            inflight: Inflight::default(),
            grants: Arc::new(Grants::load(app_data)),
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
        let network = Arc::new(Network::load(app_data, vault));
        Self {
            cookies: Arc::new(CookieJars::new(Arc::clone(&self.clients), Some(persisted))),
            oauth: Arc::new(OAuth::new(Arc::clone(&self.clients), Arc::clone(&network))),
            network,
            ..self
        }
    }

    /// OAuth 2 over this engine's clients, network settings and vault.
    pub fn oauth(&self) -> &OAuth {
        &self.oauth
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

    /// Sends a resolved draft. An OAuth 2 draft gets its stored token as Bearer auth
    /// first (plan 03b §5.3), or is refused before anything is sent. Secret values are
    /// masked in the shown URL, the log line and any error; unresolved names are
    /// reported in the response. Everything that can wait on the network, the token
    /// renewal included, runs under the request id, so Cancel stops it.
    pub async fn send_resolved(
        &self,
        request_id: &str,
        location: &RequestLocation,
        resolved: &Resolved,
    ) -> Result<ResponseMeta, AppError> {
        if let Some(error) = undefined_in_host(resolved) {
            return Err(error);
        }
        let id = validate_id(request_id)?;
        if self.responses.is_kept(&id) {
            return Err(AppError::invalid_input(
                "That request id was already used. Send again with a new id.",
            ));
        }
        let job = SendJob {
            version: self.version.clone(),
            clients: Arc::clone(&self.clients),
            cookies: Arc::clone(&self.cookies),
            network: Arc::clone(&self.network),
            oauth: Arc::clone(&self.oauth),
            grants: Arc::clone(&self.grants),
            config: self.network_config().await?,
            request_id: id.clone(),
            location: location.clone(),
            resolved: resolved.clone(),
            body_file: self.responses.path(&id)?,
            max_body_bytes: self.max_body_bytes,
        };
        let sent = self.inflight.run(&id, job.run()).await;
        if resolved.content.settings.verify_tls {
            self.cookies.changed(&jar_key(location));
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

/// One send, owned so that it runs as a cancellable task under its request id.
struct SendJob {
    version: String,
    clients: Arc<Clients>,
    cookies: Arc<CookieJars>,
    network: Arc<Network>,
    oauth: Arc<OAuth>,
    grants: Arc<Grants>,
    config: Arc<NetConfig>,
    request_id: String,
    location: RequestLocation,
    resolved: Resolved,
    body_file: PathBuf,
    max_body_bytes: u64,
}

impl SendJob {
    /// The token (if the draft uses OAuth 2), then the exchange; the result is masked.
    async fn run(self) -> Result<ResponseMeta, AppError> {
        let resolved = token_for_send(&self.oauth, &self.config, &self.location, &self.resolved)
            .await
            .map_err(|error| redact_error(error, &self.resolved))?;
        let resolved =
            with_secret(&with_digest_secret(&resolved), self.config.password()).into_owned();
        match self.exchange(&resolved).await {
            Ok(meta) => Ok(ResponseMeta {
                url: resolved.redact(&meta.url),
                unresolved: resolved.unresolved.clone(),
                ..meta
            }),
            Err(error) => Err(redact_error(error, &resolved)),
        }
    }

    async fn exchange(&self, resolved: &Resolved) -> Result<ResponseMeta, AppError> {
        let content = &resolved.content;
        validate_content(CONTENT_LABEL, content)?;
        let built = build(content, &self.version)?;
        let prepared = self.authorize(with_secret_headers(built, resolved))?;
        let settings = content.settings;
        let jar = jar_key(&self.location);
        self.cookies.open(&jar).await?;
        tracing::info!(
            method = %prepared.method,
            url = %resolved.loggable_url(),
            "sending request"
        );
        execute(Exchange {
            request_id: self.request_id.clone(),
            selector: ClientSelector::new(
                Arc::clone(&self.clients),
                self.network.vault(),
                Arc::clone(&self.config),
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
            body_file: self.body_file.clone(),
            max_body_bytes: self.max_body_bytes,
        })
        .await
    }

    /// Every file to upload must be granted; paths are replaced by their canonical form.
    fn authorize(&self, prepared: Prepared) -> Result<Prepared, AppError> {
        let scope = grant_scope(&self.location);
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
