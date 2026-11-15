//! One Postman request item to our request content (plan 09 §3 "Request mapping").

use serde_json::{Map, Value};

use super::auth::map_auth;
use super::body::map_body;
use super::vars::report_dynamics;
use crate::error::AppError;
use crate::interop::postman::report::Report;
use crate::interop::postman::schema;
use crate::store::model::RequestFile;
use crate::store::request_model::Extra;
use crate::store::script_model::SCRIPTS;
use crate::store::types::{Auth, Body, KeyValue, Pair, RequestContent, RequestKind, Settings};

const DESCRIPTION: &str = "description";
const ENABLED: &str = "enabled";
const GRAPHQL_METHOD: &str = "POST";
const WS_METHOD: &str = "GET";
const KNOWN_BEHAVIOUR: [&str; 2] = ["followRedirects", "strictSSL"];

/// Where a request item sits, for report lines.
#[derive(Debug, Clone, Copy)]
pub struct At<'a> {
    pub file: &'a str,
    pub path: &'a str,
}

/// Our content for `item` (scripts left empty: the tree adds them), with report lines.
/// `inherited` is the effective auth of the parents, used when the item has none.
pub fn map_request(
    at: At<'_>,
    item: &schema::RequestItem,
    inherited: Option<&schema::Auth>,
    report: &mut Report,
) -> RequestContent {
    let request = &item.request;
    let url = request.url.as_ref().map(url_text).unwrap_or_default();
    let headers: Vec<KeyValue> = request
        .header
        .iter()
        .map(|header| KeyValue {
            name: header.key.clone(),
            value: header.value.clone(),
            enabled: !header.disabled,
        })
        .collect();
    let content_type = headers
        .iter()
        .find(|header| header.enabled && header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.clone());
    let (body, notes) = map_body(request.body.as_ref(), content_type.as_deref());
    notes
        .into_iter()
        .for_each(|note| report.warning(at.file, &join(at.path, "request.body"), note));
    let auth = item_auth(at, request.auth.as_ref(), inherited, report);
    let kind = kind_of(&url, &body);
    let method = method_of(kind, request.method.as_deref());
    report_url_details(at, request.url.as_ref(), report);
    report_dynamics(at.file, at.path, &url, report);
    for header in &headers {
        report_dynamics(at.file, at.path, &header.value, report);
    }
    if item.response_count > 0 {
        report.info(
            at.file,
            at.path,
            format!("{} saved example(s) were dropped.", item.response_count),
        );
    }
    let body = if kind == RequestKind::Ws {
        if body != Body::None {
            report.warning(
                at.file,
                at.path,
                "A WebSocket request has no body; it was dropped.",
            );
        }
        Body::None
    } else {
        body
    };
    RequestContent {
        method,
        url,
        disabled_params: disabled_params(request.url.as_ref()),
        headers,
        body,
        auth,
        settings: settings(at, item.protocol_profile_behavior.as_ref(), report),
        kind,
        ..RequestContent::default()
    }
}

/// The request file for `content`; `description` is kept as an unknown key and scripts are
/// switched off (`enabled: false`, plan 09 D7).
pub fn request_file(
    name: &str,
    content: &RequestContent,
    description: Option<&str>,
) -> Result<RequestFile, AppError> {
    let base = RequestFile::new(name.to_string(), &content.method, content.kind);
    let file = base.with_content(content)?;
    let extra = with_description(&disable_scripts(&file.extra), description);
    let done = RequestFile { extra, ..file };
    done.validate("Request")?;
    Ok(done)
}

/// `extra` whose `scripts` section (if any) carries `enabled: false`.
pub fn disable_scripts(extra: &Extra) -> Extra {
    extra
        .iter()
        .map(|(key, value)| match (key.as_str(), value) {
            (SCRIPTS, Value::Object(section)) => {
                let mut off: Map<String, Value> = section.clone();
                off.insert(ENABLED.into(), Value::Bool(false));
                (key.clone(), Value::Object(off))
            }
            _ => (key.clone(), value.clone()),
        })
        .collect()
}

pub fn with_description(extra: &Extra, description: Option<&str>) -> Extra {
    let mut next = extra.clone();
    if let Some(text) = description.filter(|text| !text.is_empty()) {
        next.insert(DESCRIPTION.into(), Value::String(text.to_string()));
    }
    next
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

fn item_auth(
    at: At<'_>,
    own: Option<&schema::Auth>,
    inherited: Option<&schema::Auth>,
    report: &mut Report,
) -> Auth {
    let Some(source) = own.or(inherited) else {
        return Auth::None;
    };
    let (auth, note) = map_auth(source);
    if let Some(note) = note {
        report.warning(at.file, &join(at.path, "request.auth"), note);
    }
    auth
}

fn kind_of(url: &str, body: &Body) -> RequestKind {
    let lower = url.trim_start().to_ascii_lowercase();
    if lower.starts_with("ws://") || lower.starts_with("wss://") {
        RequestKind::Ws
    } else if matches!(body, Body::Graphql { .. }) {
        RequestKind::Graphql
    } else {
        RequestKind::Http
    }
}

fn method_of(kind: RequestKind, method: Option<&str>) -> String {
    match kind {
        RequestKind::Graphql => GRAPHQL_METHOD.into(),
        RequestKind::Ws => WS_METHOD.into(),
        _ => method
            .map(|m| m.trim().to_ascii_uppercase())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| "GET".into()),
    }
}

/// `raw`, or the URL rebuilt from its parts with the enabled query parameters.
fn url_text(url: &schema::Url) -> String {
    if let Some(raw) = url.raw.as_ref().filter(|raw| !raw.is_empty()) {
        return raw.clone();
    }
    let scheme = url
        .protocol
        .as_ref()
        .map(|p| format!("{p}://"))
        .unwrap_or_default();
    let port = url
        .port
        .as_ref()
        .map(|p| format!(":{p}"))
        .unwrap_or_default();
    let path = if url.path.is_empty() {
        String::new()
    } else {
        format!("/{}", url.path.join("/"))
    };
    let query: Vec<String> = url
        .query
        .iter()
        .filter(|param| !param.disabled)
        .filter_map(|param| {
            let key = param.key.as_deref()?;
            Some(match param.value.as_deref() {
                Some(value) => format!("{key}={value}"),
                None => key.to_string(),
            })
        })
        .collect();
    let query = if query.is_empty() {
        String::new()
    } else {
        format!("?{}", query.join("&"))
    };
    let hash = url
        .hash
        .as_ref()
        .map(|h| format!("#{h}"))
        .unwrap_or_default();
    format!("{scheme}{}{port}{path}{query}{hash}", url.host.join("."))
}

fn disabled_params(url: Option<&schema::Url>) -> Vec<Pair> {
    url.map(|url| {
        url.query
            .iter()
            .filter(|param| param.disabled)
            .map(|param| Pair {
                name: param.key.clone().unwrap_or_default(),
                value: param.value.clone().unwrap_or_default(),
            })
            .collect()
    })
    .unwrap_or_default()
}

fn report_url_details(at: At<'_>, url: Option<&schema::Url>, report: &mut Report) {
    for variable in url.map(|url| url.variable.as_slice()).unwrap_or_default() {
        if !variable.value.is_empty() {
            report.info(
                at.file,
                &join(at.path, "request.url.variable"),
                format!(
                    "Path variable `:{}` had a value in Postman; `:{}` stays in the URL as text.",
                    variable.key, variable.key
                ),
            );
        }
    }
}

fn settings(at: At<'_>, behaviour: Option<&Map<String, Value>>, report: &mut Report) -> Settings {
    let defaults = Settings::default();
    let Some(behaviour) = behaviour else {
        return defaults;
    };
    let flag = |key: &str, default: bool| {
        behaviour
            .get(key)
            .and_then(Value::as_bool)
            .unwrap_or(default)
    };
    let others: Vec<&str> = behaviour
        .keys()
        .map(String::as_str)
        .filter(|key| !KNOWN_BEHAVIOUR.contains(key))
        .collect();
    if !others.is_empty() {
        report.warning(
            at.file,
            &join(at.path, "protocolProfileBehavior"),
            format!(
                "{} request setting(s) are not supported and were dropped.",
                others.len()
            ),
        );
    }
    Settings {
        follow_redirects: flag("followRedirects", defaults.follow_redirects),
        verify_tls: flag("strictSSL", defaults.verify_tls),
        ..defaults
    }
}

#[cfg(test)]
#[path = "request_tests.rs"]
mod tests;
