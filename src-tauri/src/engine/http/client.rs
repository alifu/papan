//! `reqwest` clients and cookie jars (plan 03 §6). One jar per collection, one shared by
//! all scratch requests, memory only; requests sent with TLS verification off use a
//! separate jar, so cookies from an unverified server never reach a verified one.
//! Redirects are never followed by reqwest (see `redirect`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use reqwest::redirect::Policy;
use reqwest::Client;
use reqwest_cookie_store::CookieStoreMutex;

use crate::error::AppError;
use crate::store::types::RequestLocation;

const SCRATCH_JAR: &str = "scratch";
const UNVERIFIED_SUFFIX: &str = "#tls-unverified";

/// The cookie jar a location uses.
pub fn jar_key(location: &RequestLocation) -> String {
    match location {
        RequestLocation::Scratch { .. } => SCRATCH_JAR.to_string(),
        RequestLocation::Collection { collection_id, .. } => format!("collection:{collection_id}"),
    }
}

/// The jar actually used: the location's, or its separate twin when TLS is unverified.
fn effective_jar(jar: &str, verify_tls: bool) -> String {
    if verify_tls {
        jar.to_string()
    } else {
        format!("{jar}{UNVERIFIED_SUFFIX}")
    }
}

type ClientKey = (String, bool);

#[derive(Debug, Default)]
pub struct Clients {
    jars: Mutex<HashMap<String, Arc<CookieStoreMutex>>>,
    clients: Mutex<HashMap<ClientKey, Client>>,
}

fn recover<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Clients {
    fn jar(&self, key: &str) -> Arc<CookieStoreMutex> {
        Arc::clone(recover(&self.jars).entry(key.to_string()).or_default())
    }

    pub fn client(&self, jar: &str, verify_tls: bool) -> Result<Client, AppError> {
        let key = (effective_jar(jar, verify_tls), verify_tls);
        if let Some(client) = recover(&self.clients).get(&key) {
            return Ok(client.clone());
        }
        let client = Client::builder()
            .cookie_provider(self.jar(&key.0))
            .redirect(Policy::none())
            .tls_danger_accept_invalid_certs(!verify_tls)
            .build()
            .map_err(|source| {
                AppError::internal("Could not start the HTTP client.")
                    .with_detail(source.to_string())
            })?;
        recover(&self.clients).insert(key, client.clone());
        Ok(client)
    }

    /// Empties both jars of `jar` (verified and unverified); clients keep using them.
    pub fn clear(&self, jar: &str) {
        for verify_tls in [true, false] {
            let store = self.jar(&effective_jar(jar, verify_tls));
            store.lock().unwrap_or_else(PoisonError::into_inner).clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scratch_requests_share_a_jar_and_collections_have_their_own() {
        let scratch = RequestLocation::Scratch { id: "a".into() };
        let collection = RequestLocation::Collection {
            collection_id: "c1".into(),
            path: "x.http.yaml".into(),
        };

        assert_eq!(jar_key(&scratch), "scratch");
        assert_eq!(jar_key(&collection), "collection:c1");
    }

    #[test]
    fn clients_are_cached_per_jar_and_settings() {
        let clients = Clients::default();

        clients.client("a", true).unwrap();
        clients.client("a", true).unwrap();
        clients.client("a", false).unwrap();
        clients.clear("a");

        assert_eq!(recover(&clients.clients).len(), 2);
        assert_eq!(recover(&clients.jars).len(), 2);
    }
}
