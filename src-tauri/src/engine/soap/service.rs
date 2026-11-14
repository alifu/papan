//! `WsdlService`: import (walk + parse + cache), cached reads and envelopes (plan 06 §7).
//! Plain Rust over a [`Fetch`], so integration tests drive it without Tauri.

use std::path::{Path, PathBuf};

use url::Url;

use super::cache::{key, validate_key, CacheEntry, WsdlCache};
use super::envelope::envelope;
use super::model::WsdlModel;
use super::walk::{walk, Fetch, Root, WalkLimits};
use super::xml::neutralise;
use super::{
    EnvelopeOptions, SoapTemplate, WsdlImport, WsdlOperationInfo, WsdlPortInfo, WsdlServiceInfo,
};
use crate::engine::http::variables::now_ms;
use crate::error::AppError;
use crate::store::types::WsdlRef;

/// Where an import starts, already resolved and checked by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportRoot {
    /// `typed` is the URL as the user typed it (the cache key); `url` is resolved.
    Url { typed: String, url: Url },
    /// The canonical path of a file picked in the native dialog.
    File { path: PathBuf },
}

/// Masks secret values in what the import reports.
pub type Redact<'a> = &'a (dyn Fn(&str) -> String + Sync);

#[derive(Debug, Clone)]
pub struct WsdlService {
    cache: WsdlCache,
    limits: WalkLimits,
}

/// `text` (a URL, possibly with `{{variables}}`) without a user name or password.
pub fn without_credentials(text: &str) -> String {
    let Some((scheme, rest)) = text.split_once("://") else {
        return text.to_string();
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    match rest[..end].rfind('@') {
        Some(at) => format!("{scheme}://{}", &rest[at + 1..]),
        None => text.to_string(),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn services_of(model: &WsdlModel) -> Vec<WsdlServiceInfo> {
    model
        .services
        .iter()
        .map(|service| WsdlServiceInfo {
            name: service.name.clone(),
            ports: service
                .ports
                .iter()
                .map(|port| WsdlPortInfo {
                    name: port.name.clone(),
                    version: port.version,
                    address: neutralise(&port.address),
                    operations: port
                        .operations
                        .iter()
                        .map(|op| WsdlOperationInfo {
                            name: op.name.clone(),
                            action: neutralise(&op.action),
                            style: op.style,
                            documentation: neutralise(&op.documentation),
                        })
                        .collect(),
                })
                .collect(),
        })
        .collect()
}

fn import_of(id: String, entry: &CacheEntry) -> WsdlImport {
    WsdlImport {
        id,
        source: entry.source.clone(),
        ref_url: if entry.file_name.is_empty() {
            entry.source.clone()
        } else {
            String::new()
        },
        services: services_of(&entry.model),
        fetched: entry.fetched.clone(),
        warnings: entry.warnings.clone(),
    }
}

impl WsdlService {
    pub fn new(app_data: &Path) -> Self {
        Self {
            cache: WsdlCache::new(app_data),
            limits: WalkLimits::default(),
        }
    }

    /// Test-only: other walk limits.
    #[must_use]
    pub fn with_limits(self, limits: WalkLimits) -> Self {
        Self { limits, ..self }
    }

    /// Walks `root` for the request scope `scope`, caches the model and describes it.
    pub async fn import<F: Fetch>(
        &self,
        fetch: &F,
        scope: &str,
        root: ImportRoot,
        redact: Redact<'_>,
    ) -> Result<WsdlImport, AppError> {
        let (source_key, source, picked, walk_root) = match root {
            // Keyed on the source as shown: that is what the request saves as its reference.
            ImportRoot::Url { typed, url } => {
                let shown = redact(&without_credentials(&typed));
                (shown.clone(), shown, String::new(), Root::Url(url))
            }
            ImportRoot::File { path } => {
                let name = file_name(&path);
                (
                    format!("file:{}", path.display()),
                    name.clone(),
                    name,
                    Root::File(path),
                )
            }
        };
        let walked = walk(fetch, walk_root, self.limits)
            .await
            .map_err(|error| AppError {
                message: redact(&error.message),
                detail: error.detail.as_deref().map(redact),
                ..error
            })?;
        let entry = CacheEntry {
            scope: scope.to_string(),
            source,
            file_name: picked,
            fetched: walked.fetched.iter().map(|text| redact(text)).collect(),
            warnings: walked.warnings.iter().map(|text| redact(text)).collect(),
            fetched_at_ms: now_ms(),
            model: walked.model,
        };
        let id = key(scope, &source_key);
        self.cache.store(&id, &entry)?;
        tracing::info!(source = %entry.source, documents = entry.fetched.len(), "Imported a WSDL");
        Ok(import_of(id, &entry))
    }

    /// The cached import behind a saved request's reference, or `None`.
    pub fn cached(&self, scope: &str, wsdl: &WsdlRef) -> Result<Option<WsdlImport>, AppError> {
        if !wsdl.url.trim().is_empty() {
            let id = key(scope, &wsdl.url);
            return Ok(self.cache.load(&id)?.map(|entry| import_of(id, &entry)));
        }
        Ok(self
            .cache
            .find_file(scope, wsdl.file.trim())
            .map(|(id, entry)| import_of(id, &entry)))
    }

    /// The envelope of one operation of the cached import `id`.
    pub fn envelope(
        &self,
        id: &str,
        service: &str,
        port: &str,
        operation: &str,
        options: EnvelopeOptions,
    ) -> Result<SoapTemplate, AppError> {
        let entry = self.cache.load(validate_key(id)?)?.ok_or_else(|| {
            AppError::not_found("That WSDL is no longer cached. Import it again.")
        })?;
        envelope(&entry.model, service, port, operation, options)
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
