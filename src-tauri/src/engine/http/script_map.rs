//! Pure mappings between the HTTP engine and the script sandbox (plan 05 §4.2): the
//! draft to `pm.request` and back, variable scopes to script layers and back, the
//! response to `pm.response`, and the scripts that apply at a location.

use base64::Engine as _;

use super::protocol_body;
use super::resolve::Resolved;
use super::types::ResponseMeta;
use crate::error::AppError;
use crate::script::report::json_escaped;
use crate::script::{
    Layers, Scope, Script, ScriptContext, ScriptRequest, ScriptResponse, VarLayer,
};
use crate::store::types::{Body, KeyValue, RequestContent, RequestLocation, Scripts};
use crate::store::Store;
use crate::vars::persist::ScriptWrite;
use crate::vars::scopes::{Layer, Scopes, Value};
use crate::vars::types::{VarScope, VarSource};

/// The scripts of one send, outer → inner.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chains {
    pub pre_request: Vec<Script>,
    pub post_response: Vec<Script>,
}

fn has_code(scripts: &[Script]) -> bool {
    scripts
        .iter()
        .any(|script| !script.source.trim().is_empty())
}

impl Chains {
    pub fn has_pre(&self) -> bool {
        has_code(&self.pre_request)
    }

    pub fn has_post(&self) -> bool {
        has_code(&self.post_response)
    }
}

/// Every folder above a request file: `a/b/r.http.yaml` → `a`, `a/b`.
pub fn folders_of(path: &str) -> Vec<String> {
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    (1..parts.len()).map(|end| parts[..end].join("/")).collect()
}

/// Collection, each folder on the path, then the request (scratch: the request only).
/// `request` is the draft's own scripts, not the file on disk.
pub fn collect_chains(
    store: &Store,
    location: &RequestLocation,
    request: &Scripts,
) -> Result<Chains, AppError> {
    let mut sources: Vec<(String, Scripts)> = Vec::new();
    if let RequestLocation::Collection {
        collection_id,
        path,
    } = location
    {
        if !store.scripts_trusted(collection_id) {
            // Scripts of a collection that came from outside do not run until trusted.
            return Ok(Chains {
                pre_request: Vec::new(),
                post_response: Vec::new(),
            });
        }
        let collection = store.collection(collection_id)?;
        sources.push(("collection".into(), collection.read_collection_scripts()?));
        for folder in folders_of(path) {
            let scripts = collection.read_folder_scripts(&folder)?;
            sources.push((format!("folder {folder}"), scripts));
        }
    }
    sources.push(("request".into(), request.clone()));
    let active: Vec<&(String, Scripts)> = sources.iter().filter(|(_, s)| s.enabled).collect();
    Ok(Chains {
        pre_request: active
            .iter()
            .map(|(label, s)| Script::new(label.clone(), s.pre_request.clone()))
            .collect(),
        post_response: active
            .iter()
            .map(|(label, s)| Script::new(label.clone(), s.post_response.clone()))
            .collect(),
    })
}

fn var_layer(layer: &Layer) -> VarLayer {
    VarLayer {
        values: layer
            .vars
            .iter()
            .filter_map(|(name, value)| Some((name.clone(), value.text.clone()?)))
            .collect(),
        secrets: layer
            .vars
            .iter()
            .filter(|(_, value)| value.secret)
            .map(|(name, _)| name.clone())
            .collect(),
    }
}

/// The script layers of `scopes`; the environment is `None` when none is selected.
pub fn layers_of(scopes: &Scopes) -> Layers {
    let find = |source: VarSource| scopes.layers.iter().find(|l| l.source == source);
    Layers {
        globals: find(VarSource::Global).map(var_layer).unwrap_or_default(),
        collection: find(VarSource::Collection)
            .map(var_layer)
            .unwrap_or_default(),
        environment: find(VarSource::Environment).map(var_layer),
        runtime: VarLayer::default(),
    }
}

fn source_of(scope: Scope) -> VarSource {
    match scope {
        Scope::Globals => VarSource::Global,
        Scope::Collection => VarSource::Collection,
        Scope::Environment => VarSource::Environment,
        Scope::Runtime => VarSource::Runtime,
    }
}

/// `scopes` with the chain's changes applied, plus the runtime layer on top: what
/// `{{vars}}` resolve against after the pre-request chain.
pub fn scopes_after(scopes: &Scopes, context: &ScriptContext) -> Scopes {
    let changes = context.changes();
    let layers = scopes.layers.iter().map(|layer| {
        let vars = changes
            .iter()
            .filter(|change| source_of(change.scope) == layer.source)
            .fold(layer.vars.clone(), |vars, change| {
                let secret = vars.get(&change.name).is_some_and(|v| v.secret) || change.secret;
                let rest = vars.into_iter().filter(|(name, _)| name != &change.name);
                match &change.value {
                    Some(text) => {
                        let value = Value {
                            text: Some(text.clone()),
                            secret,
                        };
                        rest.chain([(change.name.clone(), value)]).collect()
                    }
                    None => rest.collect(),
                }
            });
        Layer {
            source: layer.source,
            vars,
        }
    });
    let runtime = Layer {
        source: VarSource::Runtime,
        vars: context
            .layers
            .runtime
            .values
            .iter()
            .map(|(name, text)| (name.clone(), Value::plain(text.clone())))
            .collect(),
    };
    Scopes::new(layers.chain([runtime]).collect())
}

/// Where each persistent change goes. Runtime is never persisted; a collection change
/// in a scratch request, or an environment change with none selected, has nowhere to go.
pub fn writes_of(
    context: &ScriptContext,
    location: &RequestLocation,
    env: Option<&str>,
) -> (Vec<ScriptWrite>, Vec<String>) {
    let collection_id = match location {
        RequestLocation::Collection { collection_id, .. } => Some(collection_id.as_str()),
        RequestLocation::Scratch { .. } => None,
    };
    let mut dropped = Vec::new();
    let writes = context
        .changes()
        .into_iter()
        .filter_map(|change| {
            let scope = match (change.scope, collection_id, env) {
                (Scope::Runtime, _, _) => return None,
                (Scope::Globals, _, _) => VarScope::Global,
                (Scope::Collection, Some(id), _) => VarScope::Collection {
                    collection_id: id.to_string(),
                },
                (Scope::Environment, Some(id), Some(env)) => VarScope::Environment {
                    collection_id: id.to_string(),
                    env: env.to_string(),
                },
                _ => {
                    dropped.push(format!(
                        "`{}` was kept for this send only: there is no {} to save it in.",
                        change.name,
                        if change.scope == Scope::Collection {
                            "collection"
                        } else {
                            "environment"
                        }
                    ));
                    return None;
                }
            };
            Some(ScriptWrite {
                scope,
                name: change.name,
                value: change.value,
            })
        })
        .collect();
    (writes, dropped)
}

fn form_text(fields: &[KeyValue]) -> String {
    let pairs = fields
        .iter()
        .filter(|field| field.enabled && !field.name.is_empty())
        .map(|field| (field.name.as_str(), field.value.as_str()));
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish()
}

fn body_text(body: &Body) -> Option<String> {
    match body {
        Body::Json { text } | Body::Xml { text } | Body::Text { text } => Some(text.clone()),
        Body::Form { fields } => Some(form_text(fields)),
        Body::Graphql {
            query,
            variables,
            operation_name,
        } => protocol_body::graphql_script_text(query, variables, operation_name),
        Body::None | Body::Multipart { .. } | Body::Binary { .. } | Body::Unsupported => None,
    }
}

/// `pm.request` for `content`: enabled headers, the raw (or url-encoded) body.
pub fn script_request(content: &RequestContent) -> ScriptRequest {
    ScriptRequest {
        method: content.method.clone(),
        url: content.url.clone(),
        headers: content
            .headers
            .iter()
            .filter(|row| row.enabled)
            .map(|row| (row.name.clone(), row.value.clone()))
            .collect(),
        body: body_text(&content.body),
    }
}

fn enabled(name: &str, value: &str) -> KeyValue {
    KeyValue {
        name: name.to_string(),
        value: value.to_string(),
        enabled: true,
    }
}

fn changed_body(original: &Body, text: Option<&str>) -> Body {
    let Some(text) = text else {
        return Body::None;
    };
    let text = text.to_string();
    match original {
        Body::Json { .. } => Body::Json { text },
        Body::Xml { .. } => Body::Xml { text },
        Body::Form { .. } => Body::Form {
            fields: url::form_urlencoded::parse(text.as_bytes())
                .map(|(name, value)| enabled(&name, &value))
                .collect(),
        },
        Body::Graphql { .. } => protocol_body::graphql_from_script(text),
        Body::None
        | Body::Text { .. }
        | Body::Multipart { .. }
        | Body::Binary { .. }
        | Body::Unsupported => Body::Text { text },
    }
}

/// A new draft carrying what the pre-request chain changed; `original` is untouched.
/// Parts the script did not change are kept as they were.
pub fn content_after(original: &RequestContent, request: &ScriptRequest) -> RequestContent {
    let before = script_request(original);
    if &before == request {
        return original.clone();
    }
    RequestContent {
        method: request.method.clone(),
        url: request.url.clone(),
        headers: if before.headers == request.headers {
            original.headers.clone()
        } else {
            request.headers.iter().map(|(n, v)| enabled(n, v)).collect()
        },
        body: if before.body == request.body {
            original.body.clone()
        } else {
            changed_body(&original.body, request.body.as_deref())
        },
        ..original.clone()
    }
}

/// A request built by `pm.sendRequest`: no auth, no files, the draft's settings.
pub fn content_of(request: &ScriptRequest, base: &RequestContent) -> RequestContent {
    RequestContent {
        headers: request.headers.iter().map(|(n, v)| enabled(n, v)).collect(),
        body: request
            .body
            .clone()
            .map_or(Body::None, |text| Body::Text { text }),
        settings: base.settings,
        ..RequestContent::new(request.method.clone(), request.url.clone())
    }
}

/// `pm.response` from a finished send and its (capped) body bytes.
pub fn script_response(meta: &ResponseMeta, body: &[u8]) -> ScriptResponse {
    ScriptResponse {
        code: u32::from(meta.status),
        status: meta.status_text.clone(),
        headers: meta
            .headers
            .iter()
            .map(|h| (h.name.clone(), h.value.clone()))
            .collect(),
        body: String::from_utf8_lossy(body).into_owned(),
        response_time_ms: meta.total_ms,
    }
}

/// `resolved` with `secrets` added to what its URL, errors and log line mask. A script
/// can put any secret it read into a request, where resolution does not see it.
pub fn with_secrets(resolved: Resolved, secrets: &[String]) -> Resolved {
    let added: Vec<String> = secrets
        .iter()
        .filter(|secret| !resolved.secret_values.contains(secret))
        .cloned()
        .collect();
    Resolved {
        secret_values: resolved
            .secret_values
            .iter()
            .cloned()
            .chain(added)
            .collect(),
        ..resolved
    }
}

/// Every secret value in `scopes`, with its base64 and JSON-string forms, for redaction.
pub fn secrets_of(scopes: &Scopes) -> Vec<String> {
    let values = scopes
        .layers
        .iter()
        .flat_map(|layer| layer.vars.values())
        .filter(|value| value.secret)
        .filter_map(|value| value.text.clone())
        .filter(|text| !text.is_empty());
    let mut all: Vec<String> = values
        .flat_map(|text| {
            let encoded = base64::engine::general_purpose::STANDARD.encode(&text);
            let trimmed = encoded.trim_end_matches('=').to_string();
            [json_escaped(&text), text, encoded, trimmed]
        })
        .collect();
    all.sort_unstable();
    all.dedup();
    all
}

#[cfg(test)]
#[path = "script_map_tests.rs"]
mod tests;
