//! `HttpEngine`: the plain-Rust API behind the HTTP commands (plan 03 §6). Managed by
//! Tauri as the engine state; tests drive it directly against a local server.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::build::{build, loggable_url, PartValue, Prepared, PreparedBody, PreparedPart};
use super::client::{jar_key, Clients};
use super::grants::{grant_scope, Grants};
use super::inflight::Inflight;
use super::responses::{validate_id, Responses, MAX_BODY_BYTES};
use super::send::{execute, Exchange};
use super::types::{PickedFile, ResponseBody, ResponseMeta};
use crate::error::{AppError, ErrorCode};
use crate::store::history::{Outcome, Sent};
use crate::store::request_model::validate_content;
use crate::store::types::{RequestContent, RequestLocation};
use crate::store::StoreState;

const CONTENT_LABEL: &str = "Request";
const UNNAMED: &str = "Untitled";

#[derive(Debug)]
pub struct HttpEngine {
    version: String,
    clients: Clients,
    responses: Responses,
    inflight: Inflight,
    grants: Grants,
    max_body_bytes: u64,
}

impl HttpEngine {
    /// Response bodies go under `cache_dir`; file grants live in `app_data`.
    pub fn new(cache_dir: &Path, app_data: &Path, version: &str) -> Result<Self, AppError> {
        Ok(Self {
            version: version.to_string(),
            clients: Clients::default(),
            responses: Responses::new(cache_dir)?,
            inflight: Inflight::default(),
            grants: Grants::load(app_data),
            max_body_bytes: MAX_BODY_BYTES,
        })
    }

    /// Test-only: a lower body cap.
    #[must_use]
    pub fn with_max_body_bytes(self, max_body_bytes: u64) -> Self {
        Self {
            max_body_bytes,
            ..self
        }
    }

    /// Sends the draft `content`; `location` picks the cookie jar.
    pub async fn send(
        &self,
        request_id: &str,
        location: &RequestLocation,
        content: &RequestContent,
    ) -> Result<ResponseMeta, AppError> {
        let id = validate_id(request_id)?;
        if self.responses.is_kept(&id) {
            return Err(AppError::invalid_input(
                "That request id was already used. Send again with a new id.",
            ));
        }
        validate_content(CONTENT_LABEL, content)?;
        let prepared = self.authorize(location, build(content, &self.version)?)?;
        let settings = content.settings;
        tracing::info!(
            method = %prepared.method,
            url = %loggable_url(&prepared.url),
            "sending request"
        );
        let exchange = Exchange {
            request_id: id.clone(),
            client: self
                .clients
                .client(&jar_key(location), settings.verify_tls)?,
            prepared,
            timeout: (settings.timeout_ms > 0)
                .then(|| Duration::from_millis(settings.timeout_ms.into())),
            follow_redirects: settings.follow_redirects,
            verify_tls: settings.verify_tls,
            body_file: self.responses.path(&id)?,
            max_body_bytes: self.max_body_bytes,
        };
        let meta = self.inflight.run(&id, execute(exchange)).await?;
        self.responses.keep(&id, meta.content_type.clone());
        Ok(meta)
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
        self.clients.clear(&jar_key(location));
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
