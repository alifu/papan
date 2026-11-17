//! "Get new token" (plan 03b §5.3.1, §5.4): the only way a new configuration reaches
//! the network. Client credentials ask the token endpoint directly; authorization code
//! opens the browser, waits on the loopback listener and exchanges the code with the
//! PKCE verifier. The authorize URL and the callback are never logged (hosts only).

use std::sync::Arc;

use tokio::sync::Notify;
use url::Url;

use super::super::network::NetConfig;
use super::super::resolve::Resolved;
use super::super::types::OAuthStatus;
use super::flow::{record_of, recover, MaskSet, OAuth, Target};
use super::listener::{Callback, Listener};
use super::token::{Grant, Issued};
use super::{conflict, host_of, pkce, BrowserOpener};
use crate::error::{AppError, ErrorCode};
use crate::store::types::{OAuthGrant, RequestLocation};

const CHANGED_MESSAGE: &str = "The OAuth 2 settings changed. Check them and try again.";

/// A flow id is a canonical UUID from the frontend.
fn valid_flow_id(flow_id: &str) -> Result<(), AppError> {
    let canonical =
        uuid::Uuid::parse_str(flow_id).is_ok_and(|id| id.as_hyphenated().to_string() == flow_id);
    if canonical {
        Ok(())
    } else {
        Err(AppError::invalid_input("That flow id is not valid."))
    }
}

/// The URL the browser is sent to: `auth_url` plus the request of RFC 6749 §4.1.1 and
/// RFC 7636 §4.3 (always S256).
fn authorize_url(
    target: &Target,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
) -> Result<Url, AppError> {
    let config = &target.config;
    let mut url = config
        .auth_url
        .clone()
        .ok_or_else(|| AppError::internal("The authorization URL is missing."))?;
    url.set_fragment(None);
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", &config.client_id)
            .append_pair("redirect_uri", redirect_uri);
        if !config.scope.is_empty() {
            query.append_pair("scope", &config.scope);
        }
        query
            .append_pair("state", state)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256");
    }
    Ok(url)
}

fn cancelled() -> AppError {
    AppError::new(ErrorCode::Cancelled, "The sign-in was cancelled.")
}

/// A registered "Get new token"; dropping it frees the id. It is registered before
/// anything slow (resolving variables may wait on a keychain prompt), so a cancel
/// that arrives early is kept and ends the flow before a browser opens.
pub struct PendingFlow<'a> {
    oauth: &'a OAuth,
    flow_id: String,
    cancelled: Arc<Notify>,
}

impl Drop for PendingFlow<'_> {
    fn drop(&mut self) {
        recover(&self.oauth.flows).remove(&self.flow_id);
    }
}

impl PendingFlow<'_> {
    /// Runs `work` unless the flow is, or gets, cancelled first.
    pub async fn unless_cancelled<T>(
        &self,
        work: impl std::future::Future<Output = Result<T, AppError>>,
    ) -> Result<T, AppError> {
        tokio::select! {
            biased;
            () = self.cancelled.notified() => {
                // Kept for the steps that follow: a cancelled flow stays cancelled.
                self.cancelled.notify_one();
                Err(cancelled())
            }
            outcome = work => outcome,
        }
    }
}

/// A token just issued, and the turn that keeps other requests for the same
/// configuration out until it is stored.
type Fresh = (tokio::sync::OwnedMutexGuard<()>, Issued);

impl OAuth {
    /// Registers the flow `flow_id` (a canonical UUID, one flow per id).
    pub fn begin(&self, flow_id: &str) -> Result<PendingFlow<'_>, AppError> {
        valid_flow_id(flow_id)?;
        let mut flows = recover(&self.flows);
        if flows.contains_key(flow_id) {
            return Err(conflict("That sign-in is already running."));
        }
        let cancelled = Arc::new(Notify::new());
        flows.insert(flow_id.to_string(), Arc::clone(&cancelled));
        Ok(PendingFlow {
            oauth: self,
            flow_id: flow_id.to_string(),
            cancelled,
        })
    }

    /// Ends the pending flow `flow_id`; an unknown id is a no-op.
    pub fn cancel(&self, flow_id: &str) -> Result<(), AppError> {
        valid_flow_id(flow_id)?;
        if let Some(cancelled) = recover(&self.flows).get(flow_id) {
            cancelled.notify_one();
        }
        Ok(())
    }

    /// Pending flows (for tests and diagnostics).
    pub fn pending(&self) -> usize {
        recover(&self.flows).len()
    }

    /// Takes the configuration's turn, then asks the token endpoint.
    async fn issue(
        &self,
        net: &Arc<NetConfig>,
        target: &Target,
        grant: Grant<'_>,
        mask: &mut MaskSet,
    ) -> Result<Fresh, AppError> {
        let turn = self.flight(target).lock_owned().await;
        let reply = self
            .ask(net, target, &target.config.token_url, grant)
            .await?;
        let issued = reply.issued()?;
        mask.issued(&issued);
        Ok((turn, issued))
    }

    /// Browser, loopback callback, then the code exchange.
    async fn with_browser(
        &self,
        net: &Arc<NetConfig>,
        target: &Target,
        opener: &dyn BrowserOpener,
        mask: &mut MaskSet,
    ) -> Result<Fresh, AppError> {
        let listener = Listener::bind(target.config.redirect_port).await?;
        let redirect_uri = listener.redirect_uri();
        let verifier = pkce::random_token(self.rng.as_ref())?;
        let state = pkce::random_token(self.rng.as_ref())?;
        mask.add(&verifier);
        mask.add(&state);
        let url = authorize_url(target, &redirect_uri, &state, &pkce::challenge(&verifier))?;
        tracing::info!(host = %host_of(&url), "opening the browser for an OAuth sign-in");
        opener.open(&url)?;
        // `wait` consumes the listener: the port is closed before the exchange.
        let code = match listener.wait(&state).await {
            Callback::Code(code) => code,
            Callback::Denied(text) => {
                return Err(AppError::invalid_input(format!(
                    "The provider refused the sign-in: {text}"
                )))
            }
        };
        mask.add(&code);
        let grant = Grant::Code {
            code: &code,
            verifier: &verifier,
            redirect_uri: &redirect_uri,
        };
        self.issue(net, target, grant, mask).await
    }

    /// Everything up to the token response: the part a cancel or the lifetime ends.
    async fn obtain(
        &self,
        target: &Target,
        opener: &dyn BrowserOpener,
        mask: &mut MaskSet,
    ) -> Result<Fresh, AppError> {
        let net = self.network_config().await?;
        mask.add(net.password().unwrap_or_default());
        match target.config.grant {
            OAuthGrant::AuthorizationCode => self.with_browser(&net, target, opener, mask).await,
            OAuthGrant::ClientCredentials => {
                self.issue(&net, target, Grant::ClientCredentials, mask)
                    .await
            }
        }
    }

    /// Runs "Get new token" for the configuration the dialog showed. `config_key` is
    /// the key that dialog displayed: if the draft resolves differently by now, nothing
    /// is opened or sent. Waiting for the token ends on failure, [`OAuth::cancel`] or
    /// after the flow lifetime; once the provider has issued a token it is always
    /// stored and reported as success, so the stored state and the answer agree.
    /// The returned status holds no token; errors are masked.
    pub async fn authorize_pending(
        &self,
        flow: &PendingFlow<'_>,
        location: &RequestLocation,
        resolved: &Resolved,
        config_key: &str,
        opener: &dyn BrowserOpener,
    ) -> Result<OAuthStatus, AppError> {
        let mut mask = MaskSet::of(resolved);
        let target = Target::of(location, resolved).map_err(|error| mask.mask(error))?;
        if !pkce::same(&target.key, config_key) {
            return Err(conflict(CHANGED_MESSAGE));
        }
        let waited = flow.unless_cancelled(async {
            tokio::select! {
                outcome = self.obtain(&target, opener, &mut mask) => outcome,
                () = tokio::time::sleep(self.lifetime) => {
                    Err(AppError::new(
                        ErrorCode::Timeout,
                        "The sign-in was not completed in time.",
                    ))
                }
            }
        });
        let (turn, issued) = waited.await.map_err(|error| mask.mask(error))?;
        let record = record_of(&issued, &target, None);
        let saved = self.save(&target, record.clone()).await;
        drop(turn);
        saved.map_err(|error| mask.mask(error))?;
        Ok(Self::status_of(&target, Some(&record)))
    }

    /// [`OAuth::begin`], then [`OAuth::authorize_pending`].
    pub async fn authorize(
        &self,
        flow_id: &str,
        location: &RequestLocation,
        resolved: &Resolved,
        config_key: &str,
        opener: &dyn BrowserOpener,
    ) -> Result<OAuthStatus, AppError> {
        let flow = self.begin(flow_id)?;
        self.authorize_pending(&flow, location, resolved, config_key, opener)
            .await
    }
}

#[cfg(test)]
#[path = "authorize_tests.rs"]
mod tests;
