//! A mock environment as stored in `mocks/<slug>.mock.yaml` and sent over IPC (plan 11 §2).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::AppError;
use crate::store::types::KeyValue;

pub const MAX_ROUTES: usize = 500;
pub const MAX_RESPONSES: usize = 50;
pub const MAX_RULES: usize = 50;
pub const MAX_HEADERS: usize = 100;
pub const MAX_BODY_BYTES: usize = 1024 * 1024;
pub const MAX_LATENCY_MS: u32 = 60_000;
pub const MAX_NAME_CHARS: usize = 100;
pub const MAX_BUCKETS: usize = 20;
pub const MAX_CALLBACKS: usize = 5;
pub const MAX_CALLBACK_BODY_BYTES: usize = 64 * 1024;
const MAX_FIELD_CHARS: usize = 2_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MockBind {
    #[default]
    Loopback,
    AllInterfaces,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ResponseMode {
    #[default]
    Rules,
    Sequential,
    Random,
    DisableRules,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RulesOperator {
    #[default]
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuleTarget {
    Body,
    Query,
    Header,
    Params,
    Cookie,
    RequestNumber,
    Path,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuleOperator {
    Equals,
    Regex,
    RegexI,
    Null,
    EmptyArray,
    ArrayIncludes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Rule {
    pub target: RuleTarget,
    /// Query/header/param/cookie name, or the body path (`a.b[0]`); unused for the rest.
    #[serde(default)]
    pub modifier: String,
    pub operator: RuleOperator,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub invert: bool,
}

/// What a route does: answer from its responses, or serve a data bucket as a REST resource.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RouteKind {
    #[default]
    Http,
    Crud,
    /// A WebSocket endpoint: `greeting` is sent on connect and `responses` answer each message.
    Ws,
}

/// A request sent after a response (plan 11 §11). The URL is fixed text, never templated, so a
/// caller cannot choose where it goes; only the body is templated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Callback {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<KeyValue>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub delay_ms: u32,
}

/// Named JSON that CRUD routes serve and templates read (`{{data 'name'}}`); `value` is JSON text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DataBucket {
    pub id: String,
    pub name: String,
    #[serde(default = "empty_array")]
    pub value: String,
}

fn empty_array() -> String {
    "[]".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct MockResponse {
    pub id: String,
    #[serde(default)]
    pub label: String,
    pub status: u16,
    #[serde(default)]
    pub headers: Vec<KeyValue>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub latency_ms: u32,
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub rules_operator: RulesOperator,
    #[serde(default)]
    pub default: bool,
    /// A file inside the collection to send instead of `body` (plan 11 §9); empty = use `body`.
    #[serde(default)]
    pub file: String,
    /// Requests sent after this response is chosen.
    #[serde(default)]
    pub callbacks: Vec<Callback>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct MockRoute {
    pub id: String,
    /// `get`, `post`, `put`, `patch`, `delete`, `head`, `options` or `any`.
    pub method: String,
    pub endpoint: String,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub mode: ResponseMode,
    /// `crud` routes ignore `method` and `responses` and serve `bucket` at `endpoint` and
    /// `endpoint/:id`.
    #[serde(default)]
    pub kind: RouteKind,
    /// The data bucket's id (CRUD routes).
    #[serde(default)]
    pub bucket: String,
    /// The item field that identifies an item (CRUD routes).
    #[serde(default = "default_id_key")]
    pub id_key: String,
    /// WebSocket routes: a message sent as soon as a client connects; empty = none.
    #[serde(default)]
    pub greeting: String,
    #[serde(default)]
    pub responses: Vec<MockResponse>,
}

pub fn default_id_key() -> String {
    "id".to_string()
}

fn enabled() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct MockEnvironment {
    pub name: String,
    /// 0 means any free port.
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub bind: MockBind,
    /// Path prefix without slashes at the ends, e.g. `api/v1`.
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub latency_ms: u32,
    #[serde(default = "enabled")]
    pub cors: bool,
    #[serde(default)]
    pub headers: Vec<KeyValue>,
    #[serde(default)]
    pub routes: Vec<MockRoute>,
    /// Serve HTTPS with a self-signed certificate made at start.
    #[serde(default)]
    pub tls: bool,
    /// Forward requests that match no route to this `http(s)://` base URL; empty = off.
    #[serde(default)]
    pub proxy: String,
    #[serde(default)]
    pub data: Vec<DataBucket>,
    /// Answer for requests that match no route (before the built-in 404); `rules` are ignored.
    #[serde(default)]
    pub fallback: Option<MockResponse>,
}

const METHODS: [&str; 8] = [
    "get", "post", "put", "patch", "delete", "head", "options", "any",
];

fn invalid(file: &str, field: &str, problem: &str) -> AppError {
    AppError::invalid_input(format!("{file}: {field} {problem}."))
}

fn check_text(file: &str, field: &str, text: &str, max: usize) -> Result<(), AppError> {
    if text.chars().count() > max {
        return Err(invalid(file, field, "is too long"));
    }
    Ok(())
}

/// A header name must be an HTTP token and a value must carry no control characters:
/// both end up on the wire, so a line break would split the response.
pub fn check_header(file: &str, field: &str, header: &KeyValue) -> Result<(), AppError> {
    http::HeaderName::from_bytes(header.name.as_bytes())
        .map_err(|_| invalid(file, field, "has an invalid header name"))?;
    if header.value.chars().any(|c| c.is_control() && c != '\t') {
        return Err(invalid(
            file,
            field,
            "has a header value with control characters",
        ));
    }
    Ok(())
}

/// A file reference must stay inside the collection: relative, no `..`, no hidden parts.
/// The server re-checks after resolving links; this refuses the obvious at save time.
pub fn check_file(file: &str, field: &str, path: &str) -> Result<(), AppError> {
    if path.is_empty() {
        return Ok(());
    }
    check_text(file, field, path, MAX_FIELD_CHARS)?;
    let bad = path.starts_with(['/', '\\'])
        || path.contains(['\0', ':'])
        || path
            .split(['/', '\\'])
            .any(|part| part == ".." || part.starts_with('.'));
    if bad {
        return Err(invalid(
            file,
            field,
            "must be a relative path inside the collection",
        ));
    }
    Ok(())
}

/// The proxy target must be an absolute http(s) URL without credentials.
pub fn check_proxy(file: &str, target: &str) -> Result<(), AppError> {
    if target.is_empty() {
        return Ok(());
    }
    check_http_url(file, "proxy", target)
}

/// An absolute http(s) URL without credentials: where the server may send requests.
pub fn check_http_url(file: &str, field: &str, target: &str) -> Result<(), AppError> {
    check_text(file, field, target, MAX_FIELD_CHARS)?;
    let parsed = url::Url::parse(target).map_err(|_| invalid(file, field, "is not a valid URL"))?;
    let ok = matches!(parsed.scheme(), "http" | "https")
        && parsed.host_str().is_some()
        && parsed.username().is_empty()
        && parsed.password().is_none();
    if !ok {
        return Err(invalid(
            file,
            field,
            "must be an http or https URL without credentials",
        ));
    }
    Ok(())
}

fn check_headers(file: &str, field: &str, headers: &[KeyValue]) -> Result<(), AppError> {
    if headers.len() > MAX_HEADERS {
        return Err(invalid(file, field, "has too many headers"));
    }
    headers
        .iter()
        .try_for_each(|h| check_header(file, field, h))
}

fn check_response(file: &str, at: &str, response: &MockResponse) -> Result<(), AppError> {
    if !(100..=599).contains(&response.status) {
        return Err(invalid(file, &format!("{at}.status"), "must be 100 to 599"));
    }
    if response.latency_ms > MAX_LATENCY_MS {
        return Err(invalid(file, &format!("{at}.latency_ms"), "is over 60000"));
    }
    if response.body.len() > MAX_BODY_BYTES {
        return Err(invalid(file, &format!("{at}.body"), "is over 1 MiB"));
    }
    if response.rules.len() > MAX_RULES {
        return Err(invalid(file, &format!("{at}.rules"), "has too many rules"));
    }
    check_file(file, &format!("{at}.file"), &response.file)?;
    if response.callbacks.len() > MAX_CALLBACKS {
        return Err(invalid(
            file,
            &format!("{at}.callbacks"),
            "has too many callbacks",
        ));
    }
    for (n, callback) in response.callbacks.iter().enumerate() {
        check_callback(file, &format!("{at}.callbacks[{n}]"), callback)?;
    }
    super::template::check_depth(&response.body)
        .map_err(|problem| invalid(file, &format!("{at}.body"), &problem))?;
    check_text(
        file,
        &format!("{at}.label"),
        &response.label,
        MAX_NAME_CHARS,
    )?;
    check_headers(file, &format!("{at}.headers"), &response.headers)?;
    for (n, rule) in response.rules.iter().enumerate() {
        let at = format!("{at}.rules[{n}]");
        check_text(
            file,
            &format!("{at}.modifier"),
            &rule.modifier,
            MAX_FIELD_CHARS,
        )?;
        check_text(file, &format!("{at}.value"), &rule.value, MAX_FIELD_CHARS)?;
    }
    Ok(())
}

fn check_callback(file: &str, at: &str, callback: &Callback) -> Result<(), AppError> {
    let known = METHODS[..METHODS.len() - 1].contains(&callback.method.as_str());
    if !known {
        return Err(invalid(
            file,
            &format!("{at}.method"),
            "is not a known method",
        ));
    }
    check_http_url(file, &format!("{at}.url"), &callback.url)?;
    check_headers(file, &format!("{at}.headers"), &callback.headers)?;
    if callback.body.len() > MAX_CALLBACK_BODY_BYTES {
        return Err(invalid(file, &format!("{at}.body"), "is over 64 KiB"));
    }
    super::template::check_depth(&callback.body)
        .map_err(|problem| invalid(file, &format!("{at}.body"), &problem))?;
    if callback.delay_ms > MAX_LATENCY_MS {
        return Err(invalid(file, &format!("{at}.delay_ms"), "is over 60000"));
    }
    Ok(())
}

fn check_buckets(file: &str, buckets: &[DataBucket]) -> Result<(), AppError> {
    if buckets.len() > MAX_BUCKETS {
        return Err(invalid(file, "data", "has too many data buckets"));
    }
    for (n, bucket) in buckets.iter().enumerate() {
        let at = format!("data[{n}]");
        if bucket.name.trim().is_empty() {
            return Err(invalid(file, &format!("{at}.name"), "is required"));
        }
        check_text(file, &format!("{at}.name"), &bucket.name, MAX_NAME_CHARS)?;
        if buckets[..n]
            .iter()
            .any(|other| other.name == bucket.name || other.id == bucket.id)
        {
            return Err(invalid(file, &at, "repeats a bucket name or id"));
        }
        if bucket.value.len() > MAX_BODY_BYTES {
            return Err(invalid(file, &format!("{at}.value"), "is over 1 MiB"));
        }
        serde_json::from_str::<serde_json::Value>(&bucket.value)
            .map_err(|_| invalid(file, &format!("{at}.value"), "is not valid JSON"))?;
    }
    Ok(())
}

fn check_crud(
    file: &str,
    at: &str,
    route: &MockRoute,
    buckets: &[DataBucket],
) -> Result<(), AppError> {
    if route.endpoint.trim().trim_matches('/').is_empty() {
        return Err(invalid(
            file,
            &format!("{at}.endpoint"),
            "is required for a data route",
        ));
    }
    let key = route.id_key.as_str();
    if key.is_empty() || key.chars().count() > MAX_NAME_CHARS || key.chars().any(char::is_control) {
        return Err(invalid(
            file,
            &format!("{at}.id_key"),
            "is not a usable field name",
        ));
    }
    let bucket = buckets
        .iter()
        .find(|b| b.id == route.bucket)
        .ok_or_else(|| invalid(file, &format!("{at}.bucket"), "names no data bucket"))?;
    let is_array = serde_json::from_str::<serde_json::Value>(&bucket.value)
        .map(|v| v.is_array())
        .unwrap_or(false);
    if !is_array {
        return Err(invalid(
            file,
            &format!("{at}.bucket"),
            "must hold a JSON array",
        ));
    }
    Ok(())
}

/// A WebSocket route answers with plain messages: no files, callbacks or headers to carry.
fn check_ws(file: &str, at: &str, route: &MockRoute) -> Result<(), AppError> {
    if route.greeting.len() > MAX_BODY_BYTES {
        return Err(invalid(file, &format!("{at}.greeting"), "is over 1 MiB"));
    }
    super::template::check_depth(&route.greeting)
        .map_err(|problem| invalid(file, &format!("{at}.greeting"), &problem))?;
    for (n, response) in route.responses.iter().enumerate() {
        if !response.file.is_empty() || !response.callbacks.is_empty() {
            return Err(invalid(
                file,
                &format!("{at}.responses[{n}]"),
                "cannot use a file or callbacks on a WebSocket route",
            ));
        }
    }
    Ok(())
}

/// Checks everything the server relies on; `file` names the file in messages.
pub fn validate(file: &str, mock: &MockEnvironment) -> Result<(), AppError> {
    if mock.name.trim().is_empty() {
        return Err(invalid(file, "name", "is required"));
    }
    check_text(file, "name", &mock.name, MAX_NAME_CHARS)?;
    check_text(file, "prefix", &mock.prefix, MAX_FIELD_CHARS)?;
    if mock.latency_ms > MAX_LATENCY_MS {
        return Err(invalid(file, "latency_ms", "is over 60000"));
    }
    check_headers(file, "headers", &mock.headers)?;
    check_proxy(file, &mock.proxy)?;
    check_buckets(file, &mock.data)?;
    if let Some(fallback) = &mock.fallback {
        check_response(file, "fallback", fallback)?;
    }
    if mock.routes.len() > MAX_ROUTES {
        return Err(invalid(file, "routes", "has too many routes"));
    }
    for (r, route) in mock.routes.iter().enumerate() {
        let at = format!("routes[{r}]");
        if !METHODS.contains(&route.method.as_str()) {
            return Err(invalid(
                file,
                &format!("{at}.method"),
                "is not a known method",
            ));
        }
        check_text(
            file,
            &format!("{at}.endpoint"),
            &route.endpoint,
            MAX_FIELD_CHARS,
        )?;
        if route.kind == RouteKind::Crud {
            check_crud(file, &at, route, &mock.data)?;
        }
        if route.kind == RouteKind::Ws {
            check_ws(file, &at, route)?;
        }
        let needs_responses = route.kind == RouteKind::Http;
        if (needs_responses && route.responses.is_empty()) || route.responses.len() > MAX_RESPONSES
        {
            return Err(invalid(
                file,
                &format!("{at}.responses"),
                "needs 1 to 50 responses",
            ));
        }
        for (n, response) in route.responses.iter().enumerate() {
            check_response(file, &format!("{at}.responses[{n}]"), response)?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub mod tests_support {
    use super::*;

    pub fn sample() -> MockEnvironment {
        MockEnvironment {
            name: "Users".into(),
            port: 0,
            bind: MockBind::Loopback,
            prefix: String::new(),
            latency_ms: 0,
            cors: true,
            headers: vec![],
            tls: false,
            proxy: String::new(),
            data: vec![],
            fallback: None,
            routes: vec![MockRoute {
                id: "r1".into(),
                method: "get".into(),
                endpoint: "users/:id".into(),
                enabled: true,
                mode: ResponseMode::Rules,
                kind: RouteKind::Http,
                bucket: String::new(),
                id_key: default_id_key(),
                greeting: String::new(),
                responses: vec![MockResponse {
                    id: "s1".into(),
                    label: String::new(),
                    status: 200,
                    headers: vec![],
                    body: "{}".into(),
                    latency_ms: 0,
                    rules: vec![],
                    rules_operator: RulesOperator::And,
                    default: true,
                    file: String::new(),
                    callbacks: vec![],
                }],
            }],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::sample;
    use super::*;

    #[test]
    fn a_valid_mock_passes() {
        assert!(validate("a.mock.yaml", &sample()).is_ok());
    }

    #[test]
    fn rejects_bad_fields_naming_file_and_field() {
        let mut mock = sample();
        mock.routes[0].responses[0].status = 99;
        let error = validate("a.mock.yaml", &mock).unwrap_err();
        assert!(error.message.contains("a.mock.yaml") && error.message.contains("status"));

        let mut mock = sample();
        mock.routes[0].method = "fetch".into();
        assert!(validate("f", &mock).is_err());

        let mut mock = sample();
        mock.routes[0].responses.clear();
        assert!(validate("f", &mock).is_err());

        let mut mock = sample();
        mock.latency_ms = MAX_LATENCY_MS + 1;
        assert!(validate("f", &mock).is_err());

        let mut mock = sample();
        mock.name = " ".into();
        assert!(validate("f", &mock).is_err());
    }

    #[test]
    fn file_paths_must_stay_inside_the_collection() {
        for bad in [
            "/etc/passwd",
            "../x",
            "a/../b",
            ".git/config",
            "a\\b/..\\c",
            "C:x",
            "a/.env",
        ] {
            let mut mock = sample();
            mock.routes[0].responses[0].file = bad.into();
            assert!(validate("f", &mock).is_err(), "{bad}");
        }
        let mut mock = sample();
        mock.routes[0].responses[0].file = "files/data.json".into();
        assert!(validate("f", &mock).is_ok());
    }

    #[test]
    fn proxy_must_be_an_http_url_without_credentials() {
        for bad in ["ftp://x", "not a url", "https://u:p@host", "file:///etc"] {
            let mut mock = sample();
            mock.proxy = bad.into();
            assert!(validate("f", &mock).is_err(), "{bad}");
        }
        let mut mock = sample();
        mock.proxy = "https://api.example.com/v1".into();
        assert!(validate("f", &mock).is_ok());
    }

    #[test]
    fn header_values_with_line_breaks_are_refused() {
        let mut mock = sample();
        mock.headers = vec![KeyValue {
            name: "X-A".into(),
            value: "a\r\nSet-Cookie: x=1".into(),
            enabled: true,
        }];
        assert!(validate("f", &mock).is_err());
        mock.headers[0].value = "ok".into();
        mock.headers[0].name = "bad name".into();
        assert!(validate("f", &mock).is_err());
    }

    #[test]
    fn data_routes_callbacks_and_the_fallback_are_checked() {
        let mut mock = sample();
        mock.data = vec![DataBucket {
            id: "b".into(),
            name: "users".into(),
            value: "[]".into(),
        }];
        let mut crud = mock.routes[0].clone();
        crud.kind = RouteKind::Crud;
        crud.bucket = "b".into();
        crud.responses.clear();
        mock.routes = vec![crud.clone()];
        assert!(validate("f", &mock).is_ok());

        let mut bad = mock.clone();
        bad.routes[0].bucket = "nope".into();
        assert!(validate("f", &bad).unwrap_err().message.contains("bucket"));
        let mut bad = mock.clone();
        bad.data[0].value = r#"{"a": 1}"#.into();
        assert!(validate("f", &bad).unwrap_err().message.contains("array"));
        let mut bad = mock.clone();
        bad.data[0].value = "not json".into();
        assert!(validate("f", &bad).is_err());
        let mut bad = mock.clone();
        bad.data.push(bad.data[0].clone());
        assert!(validate("f", &bad).is_err());
        let mut bad = mock.clone();
        bad.routes[0].endpoint = "/".into();
        assert!(validate("f", &bad).is_err());

        let mut http = sample();
        http.routes[0].responses[0].callbacks = vec![Callback {
            method: "post".into(),
            url: "https://hooks.example.com/x".into(),
            headers: vec![],
            body: "{}".into(),
            delay_ms: 0,
        }];
        assert!(validate("f", &http).is_ok());
        for url in ["ftp://x", "https://u:p@h", "nope"] {
            let mut bad = http.clone();
            bad.routes[0].responses[0].callbacks[0].url = url.into();
            assert!(validate("f", &bad).is_err(), "{url}");
        }
        let mut bad = http.clone();
        bad.routes[0].responses[0].callbacks[0].method = "any".into();
        assert!(validate("f", &bad).is_err());

        let mut with_fallback = sample();
        let mut fallback = with_fallback.routes[0].responses[0].clone();
        fallback.status = 99;
        with_fallback.fallback = Some(fallback);
        assert!(validate("f", &with_fallback).is_err());
    }

    #[test]
    fn websocket_routes_allow_no_responses_but_no_files_or_callbacks() {
        let mut mock = sample();
        mock.routes[0].kind = RouteKind::Ws;
        mock.routes[0].greeting = "hi {{now}}".into();
        mock.routes[0].responses.clear();
        assert!(validate("f", &mock).is_ok());
        mock.routes[0].responses = sample().routes[0].responses.clone();
        assert!(validate("f", &mock).is_ok());
        mock.routes[0].responses[0].file = "a.json".into();
        assert!(validate("f", &mock).is_err());
        mock.routes[0].responses[0].file = String::new();
        mock.routes[0].greeting = "{{#if a}}".repeat(40);
        assert!(validate("f", &mock).is_err());
    }
}
