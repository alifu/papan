//! The real [`Fetch`]: URLs through [`HttpEngine::fetch_document`] (same clients, proxy,
//! TLS and cookie jar as a send; cancellable by the import id), files through
//! [`read_confined`].

use std::path::Path;
use std::sync::Arc;

use url::Url;

use super::rules::{check_url, read_confined};
use super::walk::{Fetch, Fetched, Get};
use crate::engine::http::service::{FetchLimits, HopCheck};
use crate::engine::http::HttpEngine;
use crate::error::AppError;
use crate::store::types::{Auth, RequestLocation};

pub struct EngineFetch<'a> {
    pub engine: &'a HttpEngine,
    pub request_id: String,
    pub location: RequestLocation,
    /// The root request's resolved auth; sent only when [`Get::credentials`] allows it.
    pub auth: Auth,
}

impl Fetch for EngineFetch<'_> {
    async fn get(&self, request: Get<'_>) -> Result<Fetched, AppError> {
        let root = request.root.cloned();
        let check_hop: HopCheck = Arc::new(move |next: &Url| check_url(next, root.as_ref()));
        let limits = FetchLimits {
            max_bytes: u64::try_from(request.max_bytes).unwrap_or(u64::MAX),
            max_redirects: request.max_redirects,
            credentials: request.credentials,
            check_hop: Some(check_hop),
            ..FetchLimits::default()
        };
        let auth = if request.credentials {
            self.auth.clone()
        } else {
            Auth::None
        };
        let document = self
            .engine
            .fetch_document(&self.request_id, &self.location, request.url, &auth, limits)
            .await?;
        Ok(Fetched::Document {
            url: document.final_url,
            bytes: document.bytes,
        })
    }

    async fn read(
        &self,
        path: &Path,
        root_dir: &Path,
        max_bytes: usize,
    ) -> Result<Vec<u8>, AppError> {
        read_confined(path, root_dir, max_bytes).await
    }
}
