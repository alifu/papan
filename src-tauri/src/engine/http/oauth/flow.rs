//! The OAuth 2 service (plan 03b §5.3): token state, the send-time rule and single
//! flight per configuration. "Get new token" itself is in `authorize`.
//!
//! At send a token is only ever used, refreshed or re-requested **because a record for
//! that configuration exists**; a new configuration reaches the network through
//! [`OAuth::authorize`] alone, and Send never opens a browser.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use reqwest::Client;
use url::Url;

use super::super::client::{ClientSelector, Clients};
use super::super::network::{NetConfig, Network};
use super::super::resolve::Resolved;
use super::super::types::OAuthStatus;
use super::super::variables::now_ms;
pub(super) use super::mask_set::MaskSet;
pub use super::mask_set::MAX_ERROR_CHARS;
use super::pkce::{RandomBytes, SystemRng};
use super::token::{self, Grant, Issued, Reply};
use super::tokens::{self, Owner, TokenRecord};
use super::{config_key, host_of, listener, OAuthConfig};
use crate::error::AppError;
use crate::store::types::{Auth, HttpVersion, OAuthGrant, RequestLocation};

/// A pending "Get new token" ends after this long.
pub const FLOW_LIFETIME: Duration = Duration::from_secs(5 * 60);
/// A token this close to its expiry is treated as expired.
pub const EXPIRY_SKEW_MS: u64 = 30_000;
pub const NO_TOKEN_MESSAGE: &str = "Get a token in the Auth tab first.";
const NOT_OAUTH_MESSAGE: &str = "This request does not use OAuth 2.";

type Flight = Arc<tokio::sync::Mutex<()>>;

pub(super) fn recover<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(super) fn now() -> u64 {
    now_ms() as u64
}

/// Runs vault work off the async runtime (the first access may wait on a prompt).
pub(super) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tokio::task::spawn_blocking(work).await.map_err(|source| {
        AppError::internal("The operation failed.").with_detail(source.to_string())
    })?
}

/// One request's configuration with what names and protects its token.
pub(super) struct Target {
    pub config: OAuthConfig,
    pub key: String,
    pub owner: Owner,
    pub verify_tls: bool,
}

impl Target {
    pub(super) fn of(location: &RequestLocation, resolved: &Resolved) -> Result<Self, AppError> {
        let config = OAuthConfig::from_resolved(resolved)?
            .ok_or_else(|| AppError::invalid_input(NOT_OAUTH_MESSAGE))?;
        Ok(Self {
            key: config_key(&config),
            owner: Owner::of(location)?,
            verify_tls: resolved.content.settings.verify_tls,
            config,
        })
    }
}

fn no_token() -> AppError {
    AppError::invalid_input(NO_TOKEN_MESSAGE)
}

fn usable(record: &TokenRecord, now: u64) -> bool {
    record
        .expires_at_ms
        .is_none_or(|expires| expires > now.saturating_add(EXPIRY_SKEW_MS))
}

/// The record that replaces `previous` (if any) after `issued`. A provider that does
/// not rotate keeps the refresh token it had.
pub(super) fn record_of(
    issued: &Issued,
    target: &Target,
    previous: Option<&TokenRecord>,
) -> TokenRecord {
    let config = &target.config;
    let saved_at_ms = now();
    let kept = previous.and_then(|record| record.refresh_token.clone());
    let scope = issued
        .scope
        .clone()
        .or_else(|| previous.map(|record| record.scope.clone()))
        .unwrap_or_else(|| config.scope.clone());
    TokenRecord {
        access_token: issued.access_token.to_string(),
        refresh_token: issued
            .refresh_token
            .as_ref()
            .map(|token| token.to_string())
            .or(kept),
        expires_at_ms: issued
            .expires_in_s
            .map(|seconds| saved_at_ms.saturating_add(seconds.saturating_mul(1000))),
        scope,
        token_url: config.token_url.to_string(),
        saved_at_ms,
        // A renewal never upgrades a token first obtained without verification.
        verify_tls: target.verify_tls && previous.is_none_or(|record| record.verify_tls),
    }
}

/// `resolved` with the token as its `Bearer` auth, and the token and the client secret
/// among the masked values (§5.3.6).
fn attach(resolved: &Resolved, config: &OAuthConfig, record: &TokenRecord) -> Resolved {
    let known = &resolved.secret_values;
    let added = [&record.access_token, &config.client_secret]
        .into_iter()
        .filter(|secret| !secret.is_empty() && !known.contains(secret))
        .cloned();
    Resolved {
        content: crate::store::types::RequestContent {
            auth: Auth::Bearer {
                token: record.access_token.clone(),
            },
            ..resolved.content.clone()
        },
        secret_values: known.iter().cloned().chain(added).collect(),
        ..resolved.clone()
    }
}

/// OAuth 2 for one engine: shares its clients, network settings and vault.
pub struct OAuth {
    clients: Arc<Clients>,
    network: Arc<Network>,
    pub(super) rng: Arc<dyn RandomBytes>,
    pub(super) lifetime: Duration,
    flights: Mutex<HashMap<(Owner, String), Flight>>,
    pub(super) flows: Mutex<HashMap<String, Arc<tokio::sync::Notify>>>,
}

impl std::fmt::Debug for OAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuth")
            .field("lifetime", &self.lifetime)
            .finish_non_exhaustive()
    }
}

impl OAuth {
    pub fn new(clients: Arc<Clients>, network: Arc<Network>) -> Self {
        Self {
            clients,
            network,
            rng: Arc::new(SystemRng),
            lifetime: FLOW_LIFETIME,
            flights: Mutex::default(),
            flows: Mutex::default(),
        }
    }

    /// Test-only: a shorter flow lifetime.
    #[must_use]
    pub fn with_lifetime(self, lifetime: Duration) -> Self {
        Self { lifetime, ..self }
    }

    /// The network settings for one flow, read off the async runtime.
    pub(super) async fn network_config(&self) -> Result<Arc<NetConfig>, AppError> {
        let network = Arc::clone(&self.network);
        blocking(move || network.current()).await
    }

    pub(super) async fn load(&self, target: &Target) -> Result<Option<TokenRecord>, AppError> {
        let (vault, owner, key) = (
            self.network.vault(),
            target.owner.clone(),
            target.key.clone(),
        );
        blocking(move || tokens::load(&*vault, &owner, &key)).await
    }

    pub(super) async fn save(&self, target: &Target, record: TokenRecord) -> Result<(), AppError> {
        let (vault, owner, key) = (
            self.network.vault(),
            target.owner.clone(),
            target.key.clone(),
        );
        blocking(move || tokens::save(&*vault, &owner, &key, &record)).await
    }

    /// Deletes the record; the caller holds the flight lock.
    pub(super) async fn forget(&self, target: &Target) -> Result<(), AppError> {
        let (vault, owner, key) = (
            self.network.vault(),
            target.owner.clone(),
            target.key.clone(),
        );
        blocking(move || tokens::clear(&*vault, &owner, &key)).await
    }

    /// The lock that makes token requests for one configuration take turns.
    pub(super) fn flight(&self, target: &Target) -> Flight {
        let mut flights = recover(&self.flights);
        flights.retain(|_, flight| Arc::strong_count(flight) > 1);
        Arc::clone(
            flights
                .entry((target.owner.clone(), target.key.clone()))
                .or_default(),
        )
    }

    /// The token-endpoint client for `token_url`: the app proxy, the request's TLS
    /// setting, the client certificate that matches the host, and no cookie jar.
    async fn token_client(
        &self,
        net: &Arc<NetConfig>,
        token_url: &Url,
        verify_tls: bool,
    ) -> Result<Client, AppError> {
        ClientSelector::new(
            Arc::clone(&self.clients),
            self.network.vault(),
            Arc::clone(net),
            None,
            verify_tls,
            HttpVersion::Auto,
        )
        .for_url(token_url)
        .await
    }

    /// One token request to `token_url`.
    pub(super) async fn ask(
        &self,
        net: &Arc<NetConfig>,
        target: &Target,
        token_url: &Url,
        grant: Grant<'_>,
    ) -> Result<Reply, AppError> {
        let client = self.token_client(net, token_url, target.verify_tls).await?;
        token::send(&client, token_url, &target.config, grant).await
    }

    pub(super) fn status_of(target: &Target, record: Option<&TokenRecord>) -> OAuthStatus {
        let config = &target.config;
        let with_browser = config.grant == OAuthGrant::AuthorizationCode;
        OAuthStatus {
            config_key: target.key.clone(),
            has_token: record.is_some(),
            expires_at_ms: record
                .and_then(|record| record.expires_at_ms)
                .map(|at| at as f64),
            can_refresh: record.is_some_and(|record| {
                record.refresh_token.is_some() || config.grant == OAuthGrant::ClientCredentials
            }),
            scope: record.map_or_else(|| config.scope.clone(), |record| record.scope.clone()),
            auth_host: config.auth_url.as_ref().map(host_of),
            token_host: host_of(&config.token_url),
            plaintext: config.plaintext(),
            redirect_uri: with_browser.then(|| listener::registered_uri(config.redirect_port)),
            verify_tls: target.verify_tls,
        }
    }

    /// The token state of the draft's configuration. Touches the vault, never the network.
    pub async fn status(
        &self,
        location: &RequestLocation,
        resolved: &Resolved,
    ) -> Result<OAuthStatus, AppError> {
        let target = Target::of(location, resolved)?;
        let record = self.load(&target).await?;
        Ok(Self::status_of(&target, record.as_ref()))
    }

    /// Forgets the stored token of the draft's configuration.
    pub async fn clear(
        &self,
        location: &RequestLocation,
        resolved: &Resolved,
    ) -> Result<OAuthStatus, AppError> {
        let target = Target::of(location, resolved)?;
        // After a renewal in flight, so that it cannot store the token again.
        let flight = self.flight(&target);
        let _turn = flight.lock().await;
        self.forget(&target).await?;
        Ok(Self::status_of(&target, None))
    }

    /// The stored token, refreshed first when it has expired. Fails when there is no
    /// record, or when an expired one cannot be renewed without the browser.
    async fn current(
        &self,
        net: &Arc<NetConfig>,
        target: &Target,
        mask: &mut MaskSet,
    ) -> Result<TokenRecord, AppError> {
        let record = self.load(target).await?.ok_or_else(no_token)?;
        if usable(&record, now()) {
            return Ok(record);
        }
        let flight = self.flight(target);
        let _turn = flight.lock().await;
        // Whoever held the lock before may have renewed (or cleared) it.
        let record = self.load(target).await?.ok_or_else(no_token)?;
        if usable(&record, now()) {
            return Ok(record);
        }
        mask.add(&record.access_token);
        // Never with weaker TLS than at issue: the refresh token and the client secret
        // would go to whoever answers, with no dialog.
        if record.verify_tls && !target.verify_tls {
            return Err(no_token());
        }
        // The refresh token goes to the URL stored with it and nowhere else.
        let token_url = Url::parse(&record.token_url)
            .ok()
            .filter(|url| *url == target.config.token_url)
            .ok_or_else(no_token)?;
        let grant = match (&record.refresh_token, target.config.grant) {
            (Some(refresh_token), _) => {
                mask.add(refresh_token);
                Grant::Refresh { refresh_token }
            }
            (None, OAuthGrant::ClientCredentials) => Grant::ClientCredentials,
            (None, OAuthGrant::AuthorizationCode) => return Err(no_token()),
        };
        let reply = self.ask(net, target, &token_url, grant).await?;
        if reply.rejected() {
            // The provider will not renew this again: forget the record, so the next
            // Send asks for a new token instead of repeating the request.
            if let Err(refusal) = reply.issued() {
                let refusal = mask.mask(refusal);
                tracing::warn!("the stored OAuth token was not renewed: {refusal}");
            }
            self.forget(target).await?;
            return Err(no_token());
        }
        let issued = reply.issued()?;
        mask.issued(&issued);
        let renewed = record_of(&issued, target, Some(&record));
        self.save(target, renewed.clone()).await?;
        Ok(renewed)
    }
}

/// The send hook. A draft whose auth is not OAuth 2 comes back unchanged. Otherwise
/// the stored token of its configuration is attached as `Bearer` auth (refreshed first
/// when expired, one request at a time per configuration), or the send is refused
/// with "Get a token in the Auth tab first." when no record exists. `net` is the
/// send's own network settings. Errors are already masked.
pub async fn token_for_send(
    oauth: &OAuth,
    net: &Arc<NetConfig>,
    location: &RequestLocation,
    resolved: &Resolved,
) -> Result<Resolved, AppError> {
    if !matches!(resolved.content.auth, Auth::Oauth2 { .. }) {
        return Ok(resolved.clone());
    }
    let mut mask = MaskSet::of(resolved);
    mask.add(net.password().unwrap_or_default());
    let target = Target::of(location, resolved).map_err(|error| mask.mask(error))?;
    match oauth.current(net, &target, &mut mask).await {
        Ok(record) => Ok(attach(resolved, &target.config, &record)),
        Err(error) => Err(mask.mask(error)),
    }
}

#[cfg(test)]
#[path = "flow_tests.rs"]
mod tests;
