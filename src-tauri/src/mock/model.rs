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
    pub responses: Vec<MockResponse>,
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
    check_text(file, "proxy", target, MAX_FIELD_CHARS)?;
    let parsed =
        url::Url::parse(target).map_err(|_| invalid(file, "proxy", "is not a valid URL"))?;
    let ok = matches!(parsed.scheme(), "http" | "https")
        && parsed.host_str().is_some()
        && parsed.username().is_empty()
        && parsed.password().is_none();
    if !ok {
        return Err(invalid(
            file,
            "proxy",
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
        if route.responses.is_empty() || route.responses.len() > MAX_RESPONSES {
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
            routes: vec![MockRoute {
                id: "r1".into(),
                method: "get".into(),
                endpoint: "users/:id".into(),
                enabled: true,
                mode: ResponseMode::Rules,
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
}
