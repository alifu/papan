//! What the commands do, as plain Rust (plan 08 §6): a request draft becomes a schema
//! or a [`Prepared`] call. The store lock is never held across a network await; proto
//! files are read off the async runtime.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use prost_reflect::DescriptorPool;
use tonic::transport::Channel;

use super::cache::{CachedSet, DescriptorCache};
use super::channel::{self, TonicTransport};
use super::codec::{from_json, MAX_JSON_BYTES};
use super::connect::{call_spec, mask_error, parse_url, GrpcCallSpec};
use super::protos::ProtoCache;
use super::schema::{find_method, method_kind, schema_of};
use super::types::{GrpcMethodKind, GrpcSchema, ProtoImport};
use super::{import, reflection, GrpcEngine, Prepared};
use crate::engine::http::grants::grant_scope;
use crate::engine::http::resolve::Resolved;
use crate::engine::http::variables::{now_ms, resolve_for};
use crate::engine::http::HttpEngine;
use crate::engine::ws::handshake::masked_url;
use crate::engine::ws::messages::Masker;
use crate::error::{AppError, ErrorCode};
use crate::store::request_model::validate_content;
use crate::store::types::{
    Body, GrpcSettings, GrpcSource, RequestContent, RequestKind, RequestLocation,
};
use crate::store::StoreState;
use crate::vars::dynamic::Dynamics;
use crate::vars::VarsService;

const DEADLINE_BEFORE_START: &str = "The deadline passed before the call started.";
const CONTENT_LABEL: &str = "Request";

/// The gRPC state of the app: running calls, compiled protos, reflection cache.
pub struct GrpcState {
    pub engine: GrpcEngine,
    pub protos: Arc<ProtoCache>,
    pub cache: DescriptorCache,
}

impl GrpcState {
    pub fn new(app_data: &Path) -> Self {
        Self {
            engine: GrpcEngine::default(),
            protos: Arc::default(),
            cache: DescriptorCache::new(app_data),
        }
    }
}

/// The services the gRPC helpers work with.
pub struct GrpcDeps<'a> {
    pub http: &'a HttpEngine,
    pub store: &'a StoreState,
    pub vars: &'a VarsService,
    pub dynamics: Arc<dyn Dynamics>,
    pub state: &'a GrpcState,
    /// The app version, for the user agent.
    pub version: &'a str,
}

/// A descriptor pool and where it came from.
struct Descriptors {
    pool: DescriptorPool,
    files: Vec<String>,
    warnings: Vec<String>,
    fetched_at_ms: Option<f64>,
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tokio::task::spawn_blocking(work).await.map_err(|source| {
        AppError::internal("Could not read the proto files.").with_detail(source.to_string())
    })?
}

fn check(content: &RequestContent) -> Result<(), AppError> {
    if content.kind != RequestKind::Grpc {
        return Err(AppError::invalid_input("This is not a gRPC request."));
    }
    validate_content(CONTENT_LABEL, content)
}

impl GrpcDeps<'_> {
    async fn resolved(
        &self,
        location: &RequestLocation,
        content: &RequestContent,
    ) -> Result<Resolved, AppError> {
        let dynamics = Arc::clone(&self.dynamics);
        resolve_for(self.store, self.vars, dynamics, location, content).await
    }

    async fn protos_root(&self, location: &RequestLocation) -> Result<PathBuf, AppError> {
        let (store, location) = (Arc::clone(&self.store.store), location.clone());
        blocking(move || {
            let guard = store
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard.protos_root(&location)
        })
        .await
    }

    async fn compiled(
        &self,
        location: &RequestLocation,
        grpc: &GrpcSettings,
    ) -> Result<Descriptors, AppError> {
        let root = self.protos_root(location).await?;
        let (cache, grpc) = (Arc::clone(&self.state.protos), grpc.clone());
        let compiled =
            blocking(move || cache.compile(&root, &grpc.import_paths, &grpc.protos)).await?;
        Ok(Descriptors {
            pool: compiled.pool.clone(),
            files: compiled.files.clone(),
            warnings: Vec::new(),
            fetched_at_ms: None,
        })
    }

    fn cached(&self, location: &RequestLocation, spec: &GrpcCallSpec) -> Option<Descriptors> {
        let (scope, endpoint) = (grant_scope(location), spec.endpoint());
        let cached = self.state.cache.read(&scope, &endpoint)?;
        match reflection::pool_of(&cached.encoded) {
            Ok((pool, files)) => Some(Descriptors {
                pool,
                files,
                warnings: Vec::new(),
                fetched_at_ms: Some(cached.fetched_at_ms),
            }),
            Err(_) => {
                self.state.cache.remove(&scope, &endpoint);
                None
            }
        }
    }

    /// Fetches the descriptors over `channel` and stores them in the cache.
    async fn fetched(
        &self,
        location: &RequestLocation,
        spec: &GrpcCallSpec,
        channel: &Channel,
    ) -> Result<Descriptors, AppError> {
        let reflected = reflection::fetch(channel, spec).await?;
        let fetched_at_ms = now_ms();
        let stored = CachedSet {
            fetched_at_ms,
            encoded: reflected.encoded,
        };
        let written = self
            .state
            .cache
            .write(&grant_scope(location), &spec.endpoint(), &stored);
        let mut warnings = reflected.warnings;
        if let Err(error) = written {
            tracing::warn!(code = ?error.code, "could not cache gRPC descriptors: {error}");
            warnings.push("The descriptors could not be cached on this computer.".to_string());
        }
        Ok(Descriptors {
            pool: reflected.pool,
            files: reflected.files,
            warnings,
            fetched_at_ms: Some(fetched_at_ms),
        })
    }

    async fn spec(
        &self,
        location: &RequestLocation,
        content: &RequestContent,
    ) -> Result<(Resolved, GrpcCallSpec), AppError> {
        let resolved = self.resolved(location, content).await?;
        let spec = call_spec(self.http, location, &resolved, self.version).await?;
        Ok((resolved, spec))
    }

    /// The services of the request's server by reflection: from the cache unless
    /// `refresh`, else fetched (and cached).
    pub async fn reflect(
        &self,
        location: &RequestLocation,
        content: &RequestContent,
        refresh: bool,
    ) -> Result<GrpcSchema, AppError> {
        check(content)?;
        let (_, spec) = self.spec(location, content).await?;
        let cached = (!refresh).then(|| self.cached(location, &spec)).flatten();
        let found = match cached {
            Some(found) => found,
            None => {
                let channel = channel::connect(&spec)
                    .await
                    .map_err(|error| mask_error(error, &spec.masker()))?;
                self.fetched(location, &spec, &channel).await?
            }
        };
        as_schema(found, GrpcSource::Reflection).await
    }

    /// The reflection cache entry of the request's server, without any network, token
    /// fetch or metadata: only the URL is resolved.
    async fn cached_offline(
        &self,
        location: &RequestLocation,
        content: &RequestContent,
    ) -> Result<Option<Descriptors>, AppError> {
        let resolved = self.resolved(location, content).await?;
        if !resolved.unresolved.is_empty() {
            return Ok(None);
        }
        let Ok(url) = parse_url(&resolved.content.url) else {
            return Ok(None);
        };
        let spec = GrpcCallSpec::new(url);
        Ok(self.cached(location, &spec))
    }

    /// The services the request can call: its proto files compiled, or reflection.
    /// Reflection goes to the network (with the request's credentials) only when
    /// `allow_network`; otherwise it answers from the cache, and `None` means the user
    /// has to refresh.
    pub async fn schema(
        &self,
        location: &RequestLocation,
        content: &RequestContent,
        allow_network: bool,
    ) -> Result<Option<GrpcSchema>, AppError> {
        check(content)?;
        match content.grpc.source {
            GrpcSource::Reflection if allow_network => {
                self.reflect(location, content, false).await.map(Some)
            }
            GrpcSource::Reflection => match self.cached_offline(location, content).await? {
                Some(found) => as_schema(found, GrpcSource::Reflection).await.map(Some),
                None => Ok(None),
            },
            GrpcSource::Protos => {
                let found = self.compiled(location, &content.grpc).await?;
                as_schema(found, GrpcSource::Protos).await.map(Some)
            }
        }
    }

    /// Everything before the request goes out: variables, descriptors, the connected
    /// channel and the parsed message. Errors are masked. The deadline starts here and
    /// covers all of it; what is left of it goes to the call.
    pub async fn prepare(
        &self,
        location: &RequestLocation,
        content: &RequestContent,
    ) -> Result<Prepared, AppError> {
        let started = tokio::time::Instant::now();
        check(content)?;
        let (resolved, spec) = self.spec(location, content).await?;
        let Some(deadline) = spec.deadline else {
            return self.prepared(location, &resolved, spec, started).await;
        };
        let work = self.prepared(location, &resolved, spec, started);
        let left = deadline.saturating_sub(started.elapsed());
        tokio::time::timeout(left, work)
            .await
            .unwrap_or_else(|_| Err(AppError::new(ErrorCode::Timeout, DEADLINE_BEFORE_START)))
    }

    async fn prepared(
        &self,
        location: &RequestLocation,
        resolved: &Resolved,
        spec: GrpcCallSpec,
        started: tokio::time::Instant,
    ) -> Result<Prepared, AppError> {
        let masker = spec.masker();
        let grpc = &resolved.content.grpc;
        let local = match grpc.source {
            GrpcSource::Protos => Some(self.compiled(location, grpc).await?),
            GrpcSource::Reflection => self.cached(location, &spec),
        };
        let channel = channel::connect(&spec)
            .await
            .map_err(|error| mask_error(error, &masker))?;
        let descriptors = match local {
            Some(found) => found,
            None => self.fetched(location, &spec, &channel).await?,
        };
        let method = find_method(&descriptors.pool, &grpc.service, &grpc.method)?;
        let one_request = matches!(
            method_kind(&method),
            GrpcMethodKind::Unary | GrpcMethodKind::ServerStreaming
        );
        let first = one_request
            .then(|| from_json(&method.input(), &grpc.message))
            .transpose()
            .map_err(|error| mask_error(error, &masker))?;
        Ok(Prepared {
            transport: Box::new(TonicTransport::new(channel, method.clone(), &spec)),
            method,
            first,
            deadline: spec
                .deadline
                .map(|deadline| deadline.saturating_sub(started.elapsed())),
            masker,
            url: masked_url(&spec.url),
        })
    }

    /// `message` with the variables of `location` resolved, and a masker for the
    /// secrets it used.
    pub async fn resolve_message(
        &self,
        location: &RequestLocation,
        message: &str,
    ) -> Result<(String, Masker), AppError> {
        if message.len() > MAX_JSON_BYTES {
            return Err(AppError::invalid_input("The message is larger than 1 MiB."));
        }
        let carrier = RequestContent {
            body: Body::Text {
                text: message.to_string(),
            },
            ..RequestContent::default()
        };
        let resolved = self.resolved(location, &carrier).await?;
        if !resolved.unresolved.is_empty() {
            let names: Vec<String> = resolved
                .unresolved
                .iter()
                .map(|name| format!("`{name}`"))
                .collect();
            return Err(AppError::invalid_input(format!(
                "Not defined: {}. Select an environment or define it.",
                names.join(", ")
            )));
        }
        let masker = Masker::new(resolved.secret_values.clone());
        match resolved.content.body {
            Body::Text { text } => Ok((text, masker)),
            _ => Err(AppError::internal("The message could not be resolved.")),
        }
    }

    /// Copies `picked` and its imports into the protos folder of `location`.
    pub async fn import(
        &self,
        location: &RequestLocation,
        picked: PathBuf,
    ) -> Result<ProtoImport, AppError> {
        let root = self.protos_root(location).await?;
        blocking(move || import::import_proto(&picked, &root)).await
    }
}

/// Building the picker's schema walks every method, so it runs off the async runtime.
async fn as_schema(found: Descriptors, source: GrpcSource) -> Result<GrpcSchema, AppError> {
    blocking(move || {
        let schema = schema_of(&found.pool, source, found.files, found.fetched_at_ms);
        let mut warnings = found.warnings;
        warnings.extend(schema.warnings);
        Ok(GrpcSchema { warnings, ..schema })
    })
    .await
}
