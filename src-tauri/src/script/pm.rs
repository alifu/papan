//! The native side of `pm`: one function, string arguments only, every argument checked.
//!
//! `prelude.js` builds the friendly API on top. Nothing a script passes is trusted: the
//! operation name, the argument count, each size and each JSON shape are validated here,
//! and a rejected call throws a `TypeError` into the script without changing any state.

use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig, STANDARD};
use base64::engine::DecodePaddingMode;
use base64::{alphabet, Engine as _};
use serde::{Deserialize, Serialize};

use super::context::{
    check_size, validate_request, Scope, ScriptContext, ScriptRequest, ScriptResponse,
    MAX_BODY_BYTES, MAX_HEADERS, MAX_NAME_CHARS, MAX_VALUE_BYTES,
};
use super::report::{truncate, ConsoleLevel, ConsoleLine, ScriptPhase, TestResult};

/// Most console lines kept per chain; later lines are dropped and the report says so.
pub const MAX_CONSOLE_LINES: usize = 1000;
/// Longest console line in a report, in bytes; longer lines are cut after redaction.
pub const MAX_CONSOLE_LINE_BYTES: usize = 4 * 1024;
/// Most `pm.test` results per chain.
pub const MAX_TESTS: usize = 500;
/// Longest test name or failure message in a report, in bytes.
pub const MAX_TEST_TEXT_BYTES: usize = 4 * 1024;
/// Longest output text the host keeps before redaction. The report's own limits apply
/// after masking, so a cut there cannot leave the start of a secret behind.
pub const RAW_TEXT_BYTES: usize = 16 * 1024;
/// Largest JSON document a script may hand to the host (request meta, `sendRequest`).
const MAX_JSON_BYTES: usize = MAX_BODY_BYTES + MAX_HEADERS * (MAX_VALUE_BYTES + 1024);

const NO_ENVIRONMENT_WARNING: &str =
    "No environment is selected: pm.environment.set kept the value for this send only.";

/// How `pm.sendRequest` reaches the HTTP engine. The pipeline implements it; the sandbox
/// never opens a connection itself.
///
/// `send` blocks the script thread and must return within `remaining` (what is left of the
/// chain's wall-clock limit). The request was validated by [`validate_request`]. An `Err`
/// is handed to the script's callback as an error message, so it must not carry secrets.
pub trait SendRequestBridge: Send + Sync {
    fn send(&self, request: &ScriptRequest, remaining: Duration) -> Result<ScriptResponse, String>;
}

/// Everything the native function works on while one chain runs.
#[derive(Clone)]
pub(super) struct HostState {
    pub phase: ScriptPhase,
    pub ctx: ScriptContext,
    pub console: Vec<ConsoleLine>,
    pub console_truncated: bool,
    pub tests: Vec<TestResult>,
    pub bridge: Option<Arc<dyn SendRequestBridge>>,
    pub deadline: Instant,
    pub sends_left: u32,
    warned_no_environment: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestMeta {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SendSpec {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResponseMeta<'a> {
    code: u32,
    status: &'a str,
    headers: &'a [(String, String)],
    response_time: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<&'a str>,
}

#[derive(Serialize)]
struct SendResult<'a> {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    response: Option<ResponseMeta<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a str>,
}

type Reply = Result<Option<String>, String>;

impl HostState {
    pub fn new(
        phase: ScriptPhase,
        ctx: ScriptContext,
        bridge: Option<Arc<dyn SendRequestBridge>>,
        deadline: Instant,
        max_sends: u32,
    ) -> Self {
        Self {
            phase,
            ctx,
            console: Vec::new(),
            console_truncated: false,
            tests: Vec::new(),
            bridge,
            deadline,
            sends_left: max_sends,
            warned_no_environment: false,
        }
    }

    /// Runs one host call. `Err` becomes a `TypeError` in the script.
    pub fn dispatch(&mut self, op: &str, args: &[String]) -> Reply {
        let arg = |index: usize| args.get(index).map(String::as_str);
        match (op, args.len()) {
            ("var.get", 2) => self.var_get(args[0].as_str(), &args[1]),
            ("var.has", 2) => Ok(self
                .var_get(args[0].as_str(), &args[1])?
                .map(|_| "1".to_string())),
            ("var.all", 1) => self.var_all(&args[0]),
            ("var.set", 3) => self.var_set(&args[0], &args[1], &args[2]),
            ("var.unset", 2) => self.var_unset(&args[0], &args[1]),
            ("req.get", 0) => to_json(&RequestMeta {
                method: self.ctx.request.method.clone(),
                url: self.ctx.request.url.clone(),
                headers: self.ctx.request.headers.clone(),
            }),
            ("req.body", 0) => Ok(self.ctx.request.body.clone()),
            ("req.set", 1) => self.request_set(&args[0]),
            ("req.setBody", 0 | 1) => self.request_set_body(arg(0)),
            ("res.get", 0) => self.response_meta(),
            ("res.body", 0) => Ok(self.ctx.response.as_ref().map(|r| r.body.clone())),
            ("console", 2) => self.console(&args[0], &args[1]),
            ("test", 2 | 3) => self.test(&args[0], &args[1], arg(2)),
            ("send", 1) => self.send(&args[0]),
            ("btoa", 1) => btoa(&args[0]),
            ("atob", 1) => atob(&args[0]),
            _ => Err("Unknown host call.".into()),
        }
    }

    fn var_get(&self, scope: &str, name: &str) -> Reply {
        check_size("A variable name", name, MAX_NAME_CHARS * 4)?;
        let value = match scope {
            "any" => self.ctx.lookup(name),
            _ => self
                .ctx
                .layer(parse_scope(scope)?)
                .and_then(|layer| layer.values.get(name))
                .map(String::as_str),
        };
        Ok(value.map(str::to_string))
    }

    fn var_all(&self, scope: &str) -> Reply {
        match scope {
            "any" => to_json(&self.ctx.merged()),
            _ => match self.ctx.layer(parse_scope(scope)?) {
                Some(layer) => to_json(&layer.values),
                None => Ok(Some("{}".into())),
            },
        }
    }

    fn var_set(&mut self, scope: &str, name: &str, value: &str) -> Reply {
        let scope = self.writable_scope(parse_scope(scope)?);
        self.ctx.set(scope, name, value).map(|()| None)
    }

    fn var_unset(&mut self, scope: &str, name: &str) -> Reply {
        let scope = parse_scope(scope)?;
        if scope == Scope::Environment && self.ctx.layers.environment.is_none() {
            return Ok(None);
        }
        self.ctx.unset(scope, name).map(|()| None)
    }

    /// With no environment selected, an environment write lands in the runtime scope and
    /// the console gets one warning (plan 05 §4.1).
    fn writable_scope(&mut self, scope: Scope) -> Scope {
        if scope != Scope::Environment || self.ctx.layers.environment.is_some() {
            return scope;
        }
        if !self.warned_no_environment {
            self.warned_no_environment = true;
            self.push_console(ConsoleLevel::Warn, NO_ENVIRONMENT_WARNING);
        }
        Scope::Runtime
    }

    fn require_pre_request(&self) -> Result<(), String> {
        match self.phase {
            ScriptPhase::PreRequest => Ok(()),
            ScriptPhase::PostResponse => {
                Err("pm.request is read-only once the request has been sent.".into())
            }
        }
    }

    fn request_set(&mut self, json: &str) -> Reply {
        self.require_pre_request()?;
        let meta: RequestMeta = from_json(json)?;
        let next = ScriptRequest {
            method: meta.method,
            url: meta.url,
            headers: meta.headers,
            body: self.ctx.request.body.clone(),
        };
        validate_request(&next)?;
        self.ctx.request = next;
        Ok(None)
    }

    fn request_set_body(&mut self, body: Option<&str>) -> Reply {
        self.require_pre_request()?;
        if let Some(body) = body {
            check_size("The request body", body, MAX_BODY_BYTES)?;
        }
        self.ctx.request = ScriptRequest {
            body: body.map(str::to_string),
            ..self.ctx.request.clone()
        };
        Ok(None)
    }

    fn response_meta(&self) -> Reply {
        match &self.ctx.response {
            Some(response) => to_json(&meta_of(response, false)),
            None => Ok(None),
        }
    }

    fn console(&mut self, level: &str, text: &str) -> Reply {
        let level = match level {
            "log" => ConsoleLevel::Log,
            "info" => ConsoleLevel::Info,
            "warn" => ConsoleLevel::Warn,
            "error" => ConsoleLevel::Error,
            _ => return Err("Unknown console level.".into()),
        };
        self.push_console(level, text);
        Ok(None)
    }

    fn push_console(&mut self, level: ConsoleLevel, text: &str) {
        if self.console.len() >= MAX_CONSOLE_LINES {
            self.console_truncated = true;
            return;
        }
        self.console.push(ConsoleLine {
            level,
            text: truncate(text, RAW_TEXT_BYTES),
        });
    }

    fn test(&mut self, name: &str, passed: &str, error: Option<&str>) -> Reply {
        let passed = match passed {
            "1" => true,
            "0" => false,
            _ => return Err("Invalid test result.".into()),
        };
        if self.tests.len() >= MAX_TESTS {
            return Err(format!("A script chain records at most {MAX_TESTS} tests."));
        }
        self.tests.push(TestResult {
            name: truncate(name, RAW_TEXT_BYTES),
            passed,
            error: error
                .filter(|_| !passed)
                .map(|text| truncate(text, RAW_TEXT_BYTES)),
        });
        Ok(None)
    }

    fn send(&mut self, json: &str) -> Reply {
        let spec: SendSpec = from_json(json)?;
        let request = ScriptRequest {
            method: spec.method,
            url: spec.url,
            headers: spec.headers,
            body: spec.body,
        };
        validate_request(&request)?;
        if self.sends_left == 0 {
            return Err("pm.sendRequest: too many requests in one script chain.".into());
        }
        self.sends_left -= 1;
        let outcome = self.call_bridge(&request);
        match &outcome {
            Ok(response) => to_json(&SendResult {
                ok: true,
                response: Some(meta_of(response, true)),
                error: None,
            }),
            Err(error) => to_json(&SendResult {
                ok: false,
                response: None,
                error: Some(error),
            }),
        }
    }

    fn call_bridge(&self, request: &ScriptRequest) -> Result<ScriptResponse, String> {
        let bridge = self
            .bridge
            .as_ref()
            .ok_or("pm.sendRequest is not available here.")?;
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("The script ran out of time.".into());
        }
        let response = bridge.send(request, remaining)?;
        check_size("The response body", &response.body, MAX_BODY_BYTES)?;
        Ok(response)
    }
}

fn meta_of(response: &ScriptResponse, with_body: bool) -> ResponseMeta<'_> {
    ResponseMeta {
        code: response.code,
        status: &response.status,
        headers: &response.headers,
        response_time: response.response_time_ms,
        body: with_body.then_some(response.body.as_str()),
    }
}

fn parse_scope(scope: &str) -> Result<Scope, String> {
    match scope {
        "globals" => Ok(Scope::Globals),
        "collection" => Ok(Scope::Collection),
        "environment" => Ok(Scope::Environment),
        "runtime" => Ok(Scope::Runtime),
        _ => Err("Unknown variable scope.".into()),
    }
}

fn to_json<T: Serialize>(value: &T) -> Reply {
    serde_json::to_string(value)
        .map(Some)
        .map_err(|_| "The host could not encode a value.".to_string())
}

fn from_json<'a, T: Deserialize<'a>>(json: &'a str) -> Result<T, String> {
    check_size("The request", json, MAX_JSON_BYTES)?;
    serde_json::from_str(json).map_err(|_| "The request has an invalid shape.".to_string())
}

fn btoa(text: &str) -> Reply {
    check_size("btoa input", text, MAX_BODY_BYTES)?;
    let bytes: Option<Vec<u8>> = text
        .chars()
        .map(|c| u8::try_from(u32::from(c)).ok())
        .collect();
    let bytes = bytes.ok_or("btoa: the string contains characters outside of Latin-1.")?;
    Ok(Some(STANDARD.encode(bytes)))
}

fn atob(text: &str) -> Reply {
    const FORGIVING: GeneralPurpose = GeneralPurpose::new(
        &alphabet::STANDARD,
        GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
    );
    check_size("atob input", text, MAX_BODY_BYTES * 2)?;
    let compact: String = text.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    let bytes = FORGIVING
        .decode(compact)
        .map_err(|_| "atob: the string is not valid base64.".to_string())?;
    Ok(Some(bytes.into_iter().map(char::from).collect()))
}

#[cfg(test)]
#[path = "pm_tests.rs"]
mod tests;
