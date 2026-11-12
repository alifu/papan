//! Canonical emit of the five request sections and the merge rule of plan 03 §3:
//! defaults are omitted, unknown keys found on disk are kept, an `Unsupported` section is
//! written back verbatim, and a section the user did not change keeps its on-disk value.

use serde_json::Value;

use super::request_model::{self, Extra, Fields, AUTH, BODY, DISABLED_PARAMS, HEADERS, SECTIONS};
use super::types::{ApiKeyPlacement, Auth, Body, KeyValue, Pair, Part, RequestContent, Settings};
use crate::error::AppError;

const ENTRY_KEYS: [&str; 4] = ["enabled", "file", "name", "value"];
const BODY_KEYS: [&str; 4] = ["fields", "file", "parts", "text"];
const AUTH_KEYS: [&str; 6] = [
    "name",
    "password",
    "placement",
    "token",
    "username",
    "value",
];
const SETTINGS_KEYS: [&str; 3] = ["follow_redirects", "timeout_ms", "verify_tls"];
const TYPE_KEY: &str = "type";

/// `extra` with its five sections in canonical form (the rest untouched).
pub fn canonical_extra(label: &str, extra: &Extra) -> Result<Extra, AppError> {
    let content = request_model::parse_sections(label, extra)?;
    Ok(with_sections(extra, |key| {
        emit_section(key, &content, extra.get(key))
    }))
}

/// `extra` updated to `incoming`: a section whose typed value equals what is on disk keeps
/// its raw value (unknown keys and all); a changed one is replaced by its canonical form.
pub fn merge(label: &str, extra: &Extra, incoming: &RequestContent) -> Result<Extra, AppError> {
    let disk = request_model::parse_sections(label, extra)?;
    for key in SECTIONS {
        let changed = !section_eq(key, &disk, incoming);
        if changed && is_unsupported(key, incoming) {
            return Err(request_model::invalid(
                label,
                key,
                "was made by a newer Papan and cannot be replaced.",
            ));
        }
    }
    Ok(with_sections(extra, |key| {
        if section_eq(key, &disk, incoming) {
            extra.get(key).cloned()
        } else {
            emit_section(key, incoming, None)
        }
    }))
}

fn with_sections(extra: &Extra, section: impl Fn(&str) -> Option<Value>) -> Extra {
    extra
        .iter()
        .filter(|(key, _)| !SECTIONS.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .chain(
            SECTIONS
                .iter()
                .filter_map(|key| section(key).map(|value| (key.to_string(), value))),
        )
        .collect()
}

fn section_eq(key: &str, a: &RequestContent, b: &RequestContent) -> bool {
    match key {
        AUTH => a.auth == b.auth,
        BODY => a.body == b.body,
        DISABLED_PARAMS => a.disabled_params == b.disabled_params,
        HEADERS => a.headers == b.headers,
        _ => a.settings == b.settings,
    }
}

fn is_unsupported(key: &str, content: &RequestContent) -> bool {
    match key {
        AUTH => content.auth == Auth::Unsupported,
        BODY => content.body == Body::Unsupported,
        _ => false,
    }
}

/// The canonical value of one section, or `None` when it is all default.
/// `raw` is the on-disk value whose unknown keys are kept.
pub fn emit_section(key: &str, content: &RequestContent, raw: Option<&Value>) -> Option<Value> {
    match key {
        AUTH => emit_auth(&content.auth, raw),
        BODY => emit_body(&content.body, raw),
        DISABLED_PARAMS => list_value(content.disabled_params.iter().map(pair_fields), raw),
        HEADERS => list_value(content.headers.iter().map(key_value_fields), raw),
        _ => emit_settings(&content.settings, raw),
    }
}

/// `canonical` plus every key of `raw` that is not one of `known` (or `type`).
fn overlay(canonical: Fields, raw: Option<&Value>, known: &[&str]) -> Fields {
    let unknown = raw
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(key, _)| key.as_str() != TYPE_KEY && !known.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()));
    canonical.into_iter().chain(unknown).collect()
}

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

fn fields<const N: usize>(entries: [(&str, Option<Value>); N]) -> Fields {
    entries
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key.to_string(), value)))
        .collect()
}

fn non_empty(value: &str) -> Option<Value> {
    (!value.is_empty()).then(|| text(value))
}

fn key_value_fields(entry: &KeyValue) -> (String, Fields) {
    let entry_fields = fields([
        ("name", Some(text(&entry.name))),
        ("value", non_empty(&entry.value)),
        ("enabled", (!entry.enabled).then_some(Value::Bool(false))),
    ]);
    (entry.name.clone(), entry_fields)
}

fn pair_fields(entry: &Pair) -> (String, Fields) {
    let entry_fields = fields([
        ("name", Some(text(&entry.name))),
        ("value", non_empty(&entry.value)),
    ]);
    (entry.name.clone(), entry_fields)
}

fn part_fields(entry: &Part) -> (String, Fields) {
    let entry_fields = fields([
        ("name", Some(text(&entry.name))),
        ("value", non_empty(&entry.value)),
        ("file", entry.file.as_deref().and_then(non_empty)),
        ("enabled", (!entry.enabled).then_some(Value::Bool(false))),
    ]);
    (entry.name.clone(), entry_fields)
}

/// Entries with an empty name are dropped; the rest keep the unknown keys of the raw
/// entry at the same index (parsing is one-to-one, so indices line up).
fn list_value(
    entries: impl Iterator<Item = (String, Fields)>,
    raw: Option<&Value>,
) -> Option<Value> {
    let raw_items = raw.and_then(Value::as_array);
    let items: Vec<Value> = entries
        .enumerate()
        .filter(|(_, (name, _))| !name.is_empty())
        .map(|(index, (_, entry))| {
            let raw_entry = raw_items.and_then(|items| items.get(index));
            Value::Object(overlay(entry, raw_entry, &ENTRY_KEYS))
        })
        .collect();
    (!items.is_empty()).then_some(Value::Array(items))
}

fn typed(kind: &str, rest: Fields) -> Fields {
    std::iter::once((TYPE_KEY.to_string(), text(kind)))
        .chain(rest)
        .collect()
}

fn nested(raw: Option<&Value>, key: &str) -> Option<Value> {
    raw.and_then(|raw| raw.get(key)).cloned()
}

fn emit_body(body: &Body, raw: Option<&Value>) -> Option<Value> {
    let canonical = match body {
        Body::None => return None,
        Body::Unsupported => return raw.cloned(),
        Body::Json { text: body } => typed("json", fields([("text", Some(text(body)))])),
        Body::Xml { text: body } => typed("xml", fields([("text", Some(text(body)))])),
        Body::Text { text: body } => typed("text", fields([("text", Some(text(body)))])),
        Body::Form { fields: entries } => {
            let raw_list = nested(raw, "fields");
            let list = list_value(entries.iter().map(key_value_fields), raw_list.as_ref());
            typed("form", fields([("fields", list)]))
        }
        Body::Multipart { parts } => {
            let raw_list = nested(raw, "parts");
            let list = list_value(parts.iter().map(part_fields), raw_list.as_ref());
            typed("multipart", fields([("parts", list)]))
        }
        Body::Binary { file } => {
            let file = file.as_deref().and_then(non_empty);
            typed("binary", fields([("file", file)]))
        }
    };
    Some(Value::Object(overlay(canonical, raw, &BODY_KEYS)))
}

fn emit_auth(auth: &Auth, raw: Option<&Value>) -> Option<Value> {
    let canonical = match auth {
        Auth::None => return None,
        Auth::Unsupported => return raw.cloned(),
        Auth::Basic { username, password } => typed(
            "basic",
            fields([
                ("username", non_empty(username)),
                ("password", non_empty(password)),
            ]),
        ),
        Auth::Bearer { token } => typed("bearer", fields([("token", non_empty(token))])),
        Auth::ApiKey {
            name,
            value,
            placement,
        } => typed(
            "api_key",
            fields([
                ("name", non_empty(name)),
                ("value", non_empty(value)),
                (
                    "placement",
                    (*placement == ApiKeyPlacement::Query).then(|| text("query")),
                ),
            ]),
        ),
    };
    Some(Value::Object(overlay(canonical, raw, &AUTH_KEYS)))
}

fn emit_settings(settings: &Settings, raw: Option<&Value>) -> Option<Value> {
    let default = Settings::default();
    let canonical = fields([
        (
            "follow_redirects",
            (settings.follow_redirects != default.follow_redirects)
                .then_some(Value::Bool(settings.follow_redirects)),
        ),
        (
            "timeout_ms",
            (settings.timeout_ms != default.timeout_ms)
                .then(|| Value::Number(settings.timeout_ms.into())),
        ),
        (
            "verify_tls",
            (settings.verify_tls != default.verify_tls).then_some(Value::Bool(settings.verify_tls)),
        ),
    ]);
    let all = overlay(canonical, raw, &SETTINGS_KEYS);
    (!all.is_empty()).then_some(Value::Object(all))
}

/// `content` as it reads back after a write: unnamed entries and empty file paths dropped.
pub fn normalise(content: &RequestContent) -> RequestContent {
    let named_kv = |entries: &[KeyValue]| -> Vec<KeyValue> {
        entries
            .iter()
            .filter(|e| !e.name.is_empty())
            .cloned()
            .collect()
    };
    let file = |file: &Option<String>| file.clone().filter(|path| !path.is_empty());
    let body = match &content.body {
        Body::Form { fields } => Body::Form {
            fields: named_kv(fields),
        },
        Body::Multipart { parts } => Body::Multipart {
            parts: parts
                .iter()
                .filter(|part| !part.name.is_empty())
                .map(|part| Part {
                    file: file(&part.file),
                    ..part.clone()
                })
                .collect(),
        },
        Body::Binary { file: path } => Body::Binary { file: file(path) },
        other => other.clone(),
    };
    RequestContent {
        disabled_params: content
            .disabled_params
            .iter()
            .filter(|pair| !pair.name.is_empty())
            .cloned()
            .collect(),
        headers: named_kv(&content.headers),
        body,
        ..content.clone()
    }
}
