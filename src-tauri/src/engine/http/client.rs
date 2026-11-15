//! `reqwest` clients and cookie jars (plan 03 §6, 03b §4.3). One jar per collection, one
//! shared by all scratch requests, memory only; requests sent with TLS verification off
//! use a separate jar, so cookies from an unverified server never reach a verified one.
//! Redirects are never followed by reqwest (see `redirect`).
//!
//! A client is a function of its [`ClientKey`]: jar (none for a token endpoint), TLS
//! verification, HTTP version, client certificate and network generation (the proxy).
//! The identity is chosen per hop by [`ClientSelector`], from that hop's URL.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use reqwest::redirect::Policy;
use reqwest::{Client, Identity};
use reqwest_cookie_store::CookieStoreMutex;
use url::Url;

use super::identity;
use super::network::NetConfig;
use super::timing::{timing, Spent, TimedConnectLayer, TimedResolver, TimingLog};
use super::types::{ClientCertificate, Timing};
use crate::error::AppError;
use crate::store::types::{HttpVersion, RequestLocation};
use crate::vars::vault::SecretStore;

const SCRATCH_JAR: &str = "scratch";
const UNVERIFIED_SUFFIX: &str = "#tls-unverified";
/// Clients kept; the least recently used is dropped beyond this.
pub const MAX_CLIENTS: usize = 32;

/// The cookie jar a location uses.
pub fn jar_key(location: &RequestLocation) -> String {
    match location {
        RequestLocation::Scratch { .. } => SCRATCH_JAR.to_string(),
        RequestLocation::Collection { collection_id, .. } => format!("collection:{collection_id}"),
    }
}

/// The jar actually used: the location's, or its separate twin when TLS is unverified.
pub(super) fn effective_jar(jar: &str, verify_tls: bool) -> String {
    if verify_tls {
        jar.to_string()
    } else {
        format!("{jar}{UNVERIFIED_SUFFIX}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClientKey {
    /// The effective jar; `None` = no cookies (the OAuth token endpoint).
    pub jar: Option<String>,
    pub verify_tls: bool,
    pub http_version: HttpVersion,
    /// The client certificate id.
    pub identity: Option<String>,
    /// Stands for the proxy settings: every network-settings write bumps it.
    pub network_generation: u32,
}

/// A client and the timing log its hooks write to.
#[derive(Debug, Clone)]
pub struct Built {
    pub client: Client,
    pub timing: Arc<TimingLog>,
}

#[derive(Debug, Default)]
struct Cache {
    generation: u32,
    tick: u64,
    entries: HashMap<ClientKey, (Built, u64)>,
}

impl Cache {
    fn get(&mut self, key: &ClientKey) -> Option<Built> {
        self.tick += 1;
        let tick = self.tick;
        self.entries.get_mut(key).map(|(built, used)| {
            *used = tick;
            built.clone()
        })
    }

    fn insert(&mut self, key: ClientKey, built: Built) {
        // A slow build from older settings never displaces the newer cache.
        if key.network_generation < self.generation {
            return;
        }
        if key.network_generation != self.generation {
            self.entries.clear();
            self.generation = key.network_generation;
        }
        while self.entries.len() >= MAX_CLIENTS {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(key, _)| key.clone());
            match oldest {
                Some(oldest) => self.entries.remove(&oldest),
                None => break,
            };
        }
        self.tick += 1;
        self.entries.insert(key, (built, self.tick));
    }
}

#[derive(Debug, Default)]
pub struct Clients {
    jars: Mutex<HashMap<String, Arc<CookieStoreMutex>>>,
    cache: Mutex<Cache>,
}

fn recover<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Clients {
    pub(super) fn jar(&self, key: &str) -> Arc<CookieStoreMutex> {
        Arc::clone(recover(&self.jars).entry(key.to_string()).or_default())
    }

    /// The cached client for `key`, if any.
    fn cached(&self, key: &ClientKey) -> Option<Built> {
        recover(&self.cache).get(key)
    }

    /// The client for `key`, built with `config`'s proxy and `identity` on a miss. The
    /// caller passes the identity that `key.identity` names.
    pub fn client(
        &self,
        key: &ClientKey,
        config: &NetConfig,
        identity: Option<Identity>,
    ) -> Result<Built, AppError> {
        if let Some(built) = self.cached(key) {
            return Ok(built);
        }
        self.build_and_cache(key, config, identity)
    }

    fn build_and_cache(
        &self,
        key: &ClientKey,
        config: &NetConfig,
        identity: Option<Identity>,
    ) -> Result<Built, AppError> {
        let built = build(key, config, identity, |jar| self.jar(jar))?;
        recover(&self.cache).insert(key.clone(), built.clone());
        Ok(built)
    }

    /// Empties both jars of `jar` (verified and unverified); clients keep using them.
    pub fn clear(&self, jar: &str) {
        for verify_tls in [true, false] {
            let store = self.jar(&effective_jar(jar, verify_tls));
            store.lock().unwrap_or_else(PoisonError::into_inner).clear();
        }
    }
}

fn build(
    key: &ClientKey,
    config: &NetConfig,
    identity: Option<Identity>,
    jar: impl Fn(&str) -> Arc<CookieStoreMutex>,
) -> Result<Built, AppError> {
    let log = Arc::new(TimingLog::default());
    let builder = match key.http_version {
        HttpVersion::Auto => Client::builder(),
        HttpVersion::Http1 => Client::builder().http1_only(),
        HttpVersion::Http2 => Client::builder().http2_prior_knowledge(),
    };
    let builder = config
        .apply_proxy(builder)?
        .redirect(Policy::none())
        .tls_danger_accept_invalid_certs(!key.verify_tls)
        .dns_resolver(Arc::new(TimedResolver(Arc::clone(&log))))
        .connector_layer(TimedConnectLayer(Arc::clone(&log)));
    let builder = match &key.jar {
        Some(name) => builder.cookie_provider(jar(name)),
        None => builder,
    };
    let builder = match identity {
        Some(identity) => builder.identity(identity),
        None => builder,
    };
    // No error text: it could quote the proxy URL with its password (§15.2).
    let client = builder
        .build()
        .map_err(|_| AppError::internal("Could not start the HTTP client."))?;
    Ok(Built {
        client,
        timing: log,
    })
}

const HTTPS_PROXY_IDENTITY: &str = "A client certificate cannot be used through an HTTPS \
    proxy. Use an http:// proxy, bypass the proxy for this host, or remove the certificate.";

/// The identity of `entry`, read from the vault off the async runtime.
async fn load_identity(
    vault: Arc<dyn SecretStore>,
    entry: ClientCertificate,
) -> Result<Identity, AppError> {
    tokio::task::spawn_blocking(move || identity::load_required(&*vault, &entry))
        .await
        .map_err(|source| {
            AppError::internal("Could not load the client certificate.")
                .with_detail(source.to_string())
        })?
}

/// Picks the client for each hop of one send: same jar, TLS and version settings and
/// network configuration, with the certificate that matches the hop's URL (if any).
#[derive(Clone)]
pub struct ClientSelector {
    clients: Arc<Clients>,
    vault: Arc<dyn SecretStore>,
    config: Arc<NetConfig>,
    jar: Option<String>,
    verify_tls: bool,
    http_version: HttpVersion,
    used: Arc<Mutex<Vec<Arc<TimingLog>>>>,
}

impl std::fmt::Debug for ClientSelector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientSelector")
            .field("jar", &self.jar)
            .field("verify_tls", &self.verify_tls)
            .field("http_version", &self.http_version)
            .finish_non_exhaustive()
    }
}

impl ClientSelector {
    /// `jar` is the location's jar (`None` = no cookies).
    pub fn new(
        clients: Arc<Clients>,
        vault: Arc<dyn SecretStore>,
        config: Arc<NetConfig>,
        jar: Option<&str>,
        verify_tls: bool,
        http_version: HttpVersion,
    ) -> Self {
        Self {
            clients,
            vault,
            config,
            jar: jar.map(|jar| effective_jar(jar, verify_tls)),
            verify_tls,
            http_version,
            used: Arc::default(),
        }
    }

    pub fn config(&self) -> &NetConfig {
        &self.config
    }

    /// The client for one hop to `url`. A matching certificate is loaded from the vault
    /// (off the async runtime) only when its client has to be built; a missing value then
    /// fails the hop. A cached client keeps the identity it was built with until the
    /// network generation changes (every certificate edit bumps it) or it is evicted.
    pub async fn for_url(&self, url: &Url) -> Result<Client, AppError> {
        let matched = identity::match_entry(&self.config.certificates, url).cloned();
        if matched.is_some() && self.config.https_proxy_for(url) {
            // reqwest would present the certificate to the proxy too.
            return Err(AppError::invalid_input(HTTPS_PROXY_IDENTITY));
        }
        let key = ClientKey {
            jar: self.jar.clone(),
            verify_tls: self.verify_tls,
            http_version: self.http_version,
            identity: matched.as_ref().map(|entry| entry.id.clone()),
            network_generation: self.config.generation,
        };
        let built = match self.clients.cached(&key) {
            Some(built) => built,
            None => {
                let identity = match matched {
                    Some(entry) => Some(load_identity(Arc::clone(&self.vault), entry).await?),
                    None => None,
                };
                self.clients.build_and_cache(&key, &self.config, identity)?
            }
        };
        let mut used = recover(&self.used);
        if !used.iter().any(|log| Arc::ptr_eq(log, &built.timing)) {
            used.push(Arc::clone(&built.timing));
        }
        Ok(built.client)
    }

    /// The timing of a send that started at `started`, over every client it used.
    pub fn timing(&self, started: Instant, ttfb_ms: u32, total_ms: u32) -> Timing {
        let spent = recover(&self.used)
            .iter()
            .map(|log| log.spent_since(started))
            .fold(Spent::default(), Spent::plus);
        timing(spent, ttfb_ms, total_ms)
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
