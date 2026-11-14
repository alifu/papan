//! Introspection and the schema cache behind the GraphQL commands (plan 06 §6).
//! Introspection goes through the HTTP engine with the request's auth, cookies and network
//! settings, but runs no scripts and writes no history (§13 D3).

use std::sync::Arc;

use super::cache::SchemaCache;
use super::introspection::{introspection_content, too_large, validate_result, MAX_SCHEMA_BYTES};
use super::types::{GraphqlSchema, SchemaSource};
use crate::engine::http::build::{loggable_url, parse_url};
use crate::engine::http::grants::grant_scope;
use crate::engine::http::resolve::Resolved;
use crate::engine::http::variables::{now_ms, resolve_for};
use crate::engine::http::HttpEngine;
use crate::error::AppError;
use crate::store::types::{RequestContent, RequestLocation};
use crate::store::StoreState;
use crate::vars::dynamic::Dynamics;
use crate::vars::VarsService;

/// The services the GraphQL helpers work with.
pub struct GraphqlDeps<'a> {
    pub engine: &'a HttpEngine,
    pub store: &'a StoreState,
    pub vars: &'a VarsService,
    pub dynamics: Arc<dyn Dynamics>,
    pub cache: &'a SchemaCache,
}

/// Scheme, host, port and path of the resolved URL: the cache key and what is shown.
pub fn endpoint_of(resolved: &Resolved) -> Result<String, AppError> {
    parse_url(&resolved.content.url).map(|url| resolved.redact(&loggable_url(&url)))
}

/// `error` with every secret of `resolved` masked.
fn masked(error: AppError, resolved: &Resolved) -> AppError {
    AppError {
        message: resolved.redact(&error.message),
        detail: error.detail.map(|detail| resolved.redact(&detail)),
        ..error
    }
}

/// Introspection JSON or SDL from a picked file's bytes.
pub fn schema_from_file(bytes: &[u8]) -> Result<SchemaSource, AppError> {
    if bytes.len() > MAX_SCHEMA_BYTES {
        return Err(too_large());
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| AppError::invalid_input("The schema file is not UTF-8 text."))?;
    if text.trim_start().starts_with('{') {
        return validate_result(bytes).map(|json| SchemaSource::Introspection { json });
    }
    if text.trim().is_empty() {
        return Err(AppError::invalid_input("The schema file is empty."));
    }
    Ok(SchemaSource::Sdl {
        text: text.to_string(),
    })
}

impl GraphqlDeps<'_> {
    async fn resolved(
        &self,
        location: &RequestLocation,
        content: &RequestContent,
    ) -> Result<Resolved, AppError> {
        resolve_for(
            self.store,
            self.vars,
            Arc::clone(&self.dynamics),
            location,
            content,
        )
        .await
    }

    /// Runs the introspection query for `content`'s endpoint and caches the schema.
    pub async fn introspect(
        &self,
        request_id: &str,
        location: &RequestLocation,
        content: &RequestContent,
    ) -> Result<GraphqlSchema, AppError> {
        let resolved = self
            .resolved(location, &introspection_content(content))
            .await?;
        let endpoint = endpoint_of(&resolved)?;
        let (meta, body) = self
            .engine
            .send_for_body(request_id, location, &resolved, MAX_SCHEMA_BYTES)
            .await?;
        let body = body.ok_or_else(too_large)?;
        let json = validate_result(&body).map_err(|error| {
            let error = match meta.status {
                200..=299 => error,
                status => AppError {
                    message: format!("{} (HTTP {status})", error.message),
                    ..error
                },
            };
            masked(error, &resolved)
        })?;
        let schema = GraphqlSchema {
            endpoint,
            fetched_at_ms: now_ms(),
            source: SchemaSource::Introspection { json },
        };
        self.cache.write(&grant_scope(location), &schema)?;
        Ok(schema)
    }

    /// The cached schema of `content`'s resolved endpoint; no network.
    pub async fn read_cached(
        &self,
        location: &RequestLocation,
        content: &RequestContent,
    ) -> Result<Option<GraphqlSchema>, AppError> {
        let endpoint = endpoint_of(&self.resolved(location, content).await?)?;
        Ok(self.cache.read(&grant_scope(location), &endpoint))
    }

    pub async fn clear_cached(
        &self,
        location: &RequestLocation,
        content: &RequestContent,
    ) -> Result<(), AppError> {
        let endpoint = endpoint_of(&self.resolved(location, content).await?)?;
        self.cache.clear(&grant_scope(location), &endpoint)
    }

    /// Stores a schema read from a picked file under `content`'s endpoint.
    pub async fn store_file(
        &self,
        location: &RequestLocation,
        content: &RequestContent,
        bytes: &[u8],
    ) -> Result<GraphqlSchema, AppError> {
        let endpoint = endpoint_of(&self.resolved(location, content).await?)?;
        let schema = GraphqlSchema {
            endpoint,
            fetched_at_ms: now_ms(),
            source: schema_from_file(bytes)?,
        };
        self.cache.write(&grant_scope(location), &schema)?;
        Ok(schema)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_introspection_json_or_sdl() {
        let json = schema_from_file(br#"{"data": {"__schema": {"types": []}}}"#).unwrap();
        let sdl = schema_from_file(b"type Query { a: Int }\n").unwrap();

        assert!(matches!(json, SchemaSource::Introspection { .. }));
        assert_eq!(
            sdl,
            SchemaSource::Sdl {
                text: "type Query { a: Int }\n".into()
            }
        );
        assert!(schema_from_file(b"  \n").is_err());
        assert!(schema_from_file(&[0xff, 0xfe]).is_err());
        assert!(schema_from_file(b"{\"nope\": 1}").is_err());
    }

    #[test]
    fn the_endpoint_has_no_query_and_masks_secrets() {
        let resolved = Resolved {
            secret_values: vec!["tenant42".into()],
            ..Resolved::unchanged(RequestContent::new(
                "POST",
                "api.test/tenant42/graphql?key=k",
            ))
        };

        let endpoint = endpoint_of(&resolved).unwrap();

        assert!(!endpoint.contains("key=k"), "{endpoint}");
        assert!(!endpoint.contains("tenant42"), "{endpoint}");
        assert!(endpoint.starts_with("http://api.test/"), "{endpoint}");
    }
}
