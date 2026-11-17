//! What a script chain can see and change: variable layers, the request, the response.
//!
//! Everything here is plain data. The sandbox works on a copy and hands it back when the
//! chain ends, so a failure inside the engine leaves the caller's context untouched.

use std::collections::{BTreeMap, BTreeSet};

/// Longest variable name a script may write, in characters.
pub const MAX_NAME_CHARS: usize = 128;
/// Largest variable value, URL or header value a script may write, in bytes.
pub const MAX_VALUE_BYTES: usize = 64 * 1024;
/// Most variables one scope may hold after a script write.
pub const MAX_VARS_PER_SCOPE: usize = 1000;
/// Most bytes (names + values) one scope may hold after a script write.
pub const MAX_SCOPE_BYTES: usize = 4 * 1024 * 1024;
/// Largest request or response body a script may read or write, in bytes.
pub const MAX_BODY_BYTES: usize = 5 * 1024 * 1024;
/// Most headers a script-built request may carry.
pub const MAX_HEADERS: usize = 200;
const MAX_METHOD_CHARS: usize = 32;

/// The variable scopes `pm` exposes, lowest priority first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    Globals,
    Collection,
    Environment,
    /// Lives for one send only; never persisted.
    Runtime,
}

/// One scope's variables. `secrets` names the variables declared secret in that scope.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VarLayer {
    pub values: BTreeMap<String, String>,
    pub secrets: BTreeSet<String>,
}

impl VarLayer {
    fn bytes(&self) -> usize {
        self.values.iter().map(|(k, v)| k.len() + v.len()).sum()
    }
}

/// All scopes. `environment` is `None` when no environment is selected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layers {
    pub globals: VarLayer,
    pub collection: VarLayer,
    pub environment: Option<VarLayer>,
    pub runtime: VarLayer,
}

/// The request as a script sees it. The pipeline maps it from and to `RequestContent`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

/// The response as a script sees it (`pm.response`, and the result of `pm.sendRequest`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptResponse {
    pub code: u32,
    /// Reason phrase, e.g. "OK".
    pub status: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub response_time_ms: u32,
}

/// A variable a script chain changed. `value` is `None` for an unset.
///
/// Carries the real value, secrets included: it is for persistence, never for the webview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarChange {
    pub scope: Scope,
    pub name: String,
    pub value: Option<String>,
    /// The variable is declared secret in its scope: the value belongs in the vault.
    pub secret: bool,
}

/// The state shared by every script of a send: pre-request and post-response chains
/// run against the same context, so `changes` covers both.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptContext {
    pub layers: Layers,
    pub request: ScriptRequest,
    pub response: Option<ScriptResponse>,
    /// Secret values a script never reads through `pm` but that must still not reach the
    /// report (e.g. auth secrets resolved into the request).
    pub extra_secrets: Vec<String>,
    baseline: Layers,
}

impl ScriptContext {
    pub fn new(layers: Layers, request: ScriptRequest) -> Self {
        Self {
            baseline: layers.clone(),
            layers,
            request,
            response: None,
            extra_secrets: Vec::new(),
        }
    }

    /// The layer of `scope`, or `None` for the environment when none is selected.
    pub fn layer(&self, scope: Scope) -> Option<&VarLayer> {
        layer_of(&self.layers, scope)
    }

    /// Highest priority first: runtime, environment, collection, globals.
    pub fn lookup(&self, name: &str) -> Option<&str> {
        [
            Scope::Runtime,
            Scope::Environment,
            Scope::Collection,
            Scope::Globals,
        ]
        .into_iter()
        .filter_map(|scope| self.layer(scope))
        .find_map(|layer| layer.values.get(name))
        .map(String::as_str)
    }

    /// Every visible variable, higher scopes overriding lower ones.
    pub fn merged(&self) -> BTreeMap<String, String> {
        [
            Scope::Globals,
            Scope::Collection,
            Scope::Environment,
            Scope::Runtime,
        ]
        .into_iter()
        .filter_map(|scope| self.layer(scope))
        .flat_map(|layer| layer.values.clone())
        .collect()
    }

    /// Writes a variable. Fails (leaving the context as it was) when the name or value
    /// breaks a limit, or when `scope` is the environment and none is selected.
    pub fn set(&mut self, scope: Scope, name: &str, value: &str) -> Result<(), String> {
        check_name(name)?;
        check_size("Variable value", value, MAX_VALUE_BYTES)?;
        let layer = self.layer_mut(scope)?;
        let is_new = !layer.values.contains_key(name);
        if is_new && layer.values.len() >= MAX_VARS_PER_SCOPE {
            return Err(format!(
                "A scope holds at most {MAX_VARS_PER_SCOPE} variables."
            ));
        }
        let old = layer.values.get(name).map_or(0, String::len);
        let grown = layer.bytes() - old + value.len() + if is_new { name.len() } else { 0 };
        if value.len() > old && grown > MAX_SCOPE_BYTES {
            return Err(format!(
                "A scope holds at most {MAX_SCOPE_BYTES} bytes of variables."
            ));
        }
        layer.values.insert(name.to_string(), value.to_string());
        Ok(())
    }

    pub fn unset(&mut self, scope: Scope, name: &str) -> Result<(), String> {
        check_name(name)?;
        self.layer_mut(scope)?.values.remove(name);
        Ok(())
    }

    fn layer_mut(&mut self, scope: Scope) -> Result<&mut VarLayer, String> {
        match scope {
            Scope::Globals => Ok(&mut self.layers.globals),
            Scope::Collection => Ok(&mut self.layers.collection),
            Scope::Runtime => Ok(&mut self.layers.runtime),
            Scope::Environment => self
                .layers
                .environment
                .as_mut()
                .ok_or_else(|| "No environment is selected.".to_string()),
        }
    }

    /// Everything the scripts changed since the context was created, by scope then name.
    pub fn changes(&self) -> Vec<VarChange> {
        diff(&self.baseline, &self.layers)
    }

    /// Every secret value the context knows, before and after the scripts ran.
    pub fn secret_values(&self) -> Vec<String> {
        let layers = [&self.baseline, &self.layers];
        let from_layers = layers.into_iter().flat_map(|layers| {
            ALL_SCOPES
                .into_iter()
                .filter_map(|scope| layer_of(layers, scope))
                .flat_map(|layer| {
                    layer
                        .secrets
                        .iter()
                        .filter_map(|name| layer.values.get(name).cloned())
                })
        });
        let unique: BTreeSet<String> = from_layers
            .chain(self.extra_secrets.iter().cloned())
            .filter(|value| !value.is_empty())
            .collect();
        unique.into_iter().collect()
    }
}

const ALL_SCOPES: [Scope; 4] = [
    Scope::Globals,
    Scope::Collection,
    Scope::Environment,
    Scope::Runtime,
];

fn layer_of(layers: &Layers, scope: Scope) -> Option<&VarLayer> {
    match scope {
        Scope::Globals => Some(&layers.globals),
        Scope::Collection => Some(&layers.collection),
        Scope::Environment => layers.environment.as_ref(),
        Scope::Runtime => Some(&layers.runtime),
    }
}

/// The variable changes that turn `before` into `after`, by scope then name.
pub fn diff(before: &Layers, after: &Layers) -> Vec<VarChange> {
    let empty = VarLayer::default();
    ALL_SCOPES
        .into_iter()
        .flat_map(|scope| {
            let old = layer_of(before, scope).unwrap_or(&empty);
            let new = layer_of(after, scope).unwrap_or(&empty);
            let names: BTreeSet<&String> = old.values.keys().chain(new.values.keys()).collect();
            names
                .into_iter()
                .filter(|name| old.values.get(*name) != new.values.get(*name))
                .map(|name| VarChange {
                    scope,
                    name: name.clone(),
                    value: new.values.get(name).cloned(),
                    secret: new.secrets.contains(name) || old.secrets.contains(name),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(super) fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("A variable name cannot be empty.".into());
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!(
            "A variable name is at most {MAX_NAME_CHARS} characters."
        ));
    }
    if name.chars().any(char::is_control) {
        return Err("A variable name cannot contain control characters.".into());
    }
    Ok(())
}

pub(super) fn check_size(what: &str, text: &str, max: usize) -> Result<(), String> {
    if text.len() > max {
        return Err(format!("{what} is larger than {max} bytes."));
    }
    Ok(())
}

fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// Checks a request a script built or changed before the host accepts it.
pub fn validate_request(request: &ScriptRequest) -> Result<(), String> {
    if request.method.len() > MAX_METHOD_CHARS || !is_token(&request.method) {
        return Err("The request method is not a valid HTTP method.".into());
    }
    check_size("The request URL", &request.url, MAX_VALUE_BYTES)?;
    if request.url.chars().any(char::is_control) {
        return Err("The request URL cannot contain control characters.".into());
    }
    if request.headers.len() > MAX_HEADERS {
        return Err(format!("A request holds at most {MAX_HEADERS} headers."));
    }
    for (name, value) in &request.headers {
        if name.len() > MAX_NAME_CHARS || !is_token(name) {
            return Err("A header name is not a valid HTTP header name.".into());
        }
        check_size("A header value", value, MAX_VALUE_BYTES)?;
        if value.bytes().any(|b| matches!(b, b'\r' | b'\n' | 0)) {
            return Err(format!("Header `{name}` cannot contain a line break."));
        }
    }
    match &request.body {
        Some(body) => check_size("The request body", body, MAX_BODY_BYTES),
        None => Ok(()),
    }
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
