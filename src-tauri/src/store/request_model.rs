//! The nine request sections of plan 03 §3, plan 05 §3 and plan 06 §3 (`auth`, `body`,
//! `disabled_params`, `headers`, `scripts`, `settings`, `soap`): parsing from the raw YAML
//! values and validation of the typed content. Emitting and the merge rule live in
//! `request_emit`; the protocol parts (`kind`, GraphQL, SOAP) in `protocol_model`.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::auth_model::{parse_auth, validate_auth};
use super::model;
use super::protocol_model::{self, SOAP};
use super::script_model::{parse_scripts, validate_scripts, SCRIPTS};
use super::subscription_model::{self, SUBSCRIPTION};
use super::types::{
    Body, HttpVersion, KeyValue, Pair, Part, RequestContent, RequestKind, Settings,
    DEFAULT_TIMEOUT_MS,
};
use super::ws_model::{self, WS};
use super::yaml;
use crate::error::AppError;

pub const AUTH: &str = "auth";
pub const BODY: &str = "body";
pub const DISABLED_PARAMS: &str = "disabled_params";
pub const HEADERS: &str = "headers";
pub const SETTINGS: &str = "settings";
pub const SECTIONS: [&str; 9] = [
    AUTH,
    BODY,
    DISABLED_PARAMS,
    HEADERS,
    SCRIPTS,
    SETTINGS,
    SOAP,
    SUBSCRIPTION,
    WS,
];

pub const MAX_ENTRIES: usize = 200;
pub const MAX_PARTS: usize = 100;
pub const MAX_ENTRY_NAME_CHARS: usize = 256;
pub const MAX_ENTRY_VALUE_CHARS: usize = 8192;
pub const MAX_BODY_TEXT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_TIMEOUT_MS: u32 = 600_000;
const MAX_FILE_PATH_CHARS: usize = 4096;

pub type Extra = BTreeMap<String, Value>;
pub type Fields = Map<String, Value>;

/// `invalid_input` naming the file and the (dotted) field.
pub fn invalid(label: &str, field: &str, problem: &str) -> AppError {
    yaml::invalid(label, &format!("field `{field}` {problem}"))
}

/// The typed sections of a request file of `kind`. `method` and `url` are left empty.
pub fn parse_sections(
    label: &str,
    kind: RequestKind,
    extra: &Extra,
) -> Result<RequestContent, AppError> {
    let content = RequestContent {
        method: String::new(),
        url: String::new(),
        disabled_params: parse_list(
            label,
            extra.get(DISABLED_PARAMS),
            DISABLED_PARAMS,
            parse_pair,
        )?,
        headers: parse_list(label, extra.get(HEADERS), HEADERS, parse_key_value)?,
        body: protocol_model::body_for_kind(kind, parse_body(label, extra.get(BODY))?),
        auth: parse_auth(label, extra.get(AUTH))?,
        settings: parse_settings(label, extra.get(SETTINGS))?,
        scripts: parse_scripts(label, extra.get(SCRIPTS))?,
        kind,
        soap: protocol_model::parse_soap(label, kind, extra.get(SOAP))?,
        ws: ws_model::parse_ws(label, kind, extra.get(WS))?,
        subscription: subscription_model::parse_subscription(label, kind, extra.get(SUBSCRIPTION))?,
    };
    validate_sections(label, &content)?;
    Ok(content)
}

/// Checks typed content from the webview: shared by `write_request` and `send_request`.
pub fn validate_content(label: &str, content: &RequestContent) -> Result<(), AppError> {
    protocol_model::check_method(label, content.kind, &content.method)?;
    model::check_url(label, &content.url)?;
    protocol_model::check_url(label, content.kind, &content.url)?;
    validate_sections(label, content)
}

pub(super) fn present(value: Option<&Value>) -> Option<&Value> {
    value.filter(|value| !value.is_null())
}

pub(super) fn as_map<'a>(
    label: &str,
    value: &'a Value,
    field: &str,
) -> Result<&'a Fields, AppError> {
    value
        .as_object()
        .ok_or_else(|| invalid(label, field, "must be a mapping."))
}

type ParseEntry<T> = fn(&str, &Fields, &str) -> Result<T, AppError>;

fn parse_list<T>(
    label: &str,
    value: Option<&Value>,
    field: &str,
    parse: ParseEntry<T>,
) -> Result<Vec<T>, AppError> {
    let Some(value) = present(value) else {
        return Ok(Vec::new());
    };
    let items = value
        .as_array()
        .ok_or_else(|| invalid(label, field, "must be a list."))?;
    items
        .iter()
        .map(|item| parse(label, as_map(label, item, field)?, field))
        .collect()
}

pub(super) fn opt_text(
    label: &str,
    fields: &Fields,
    prefix: &str,
    key: &str,
) -> Result<Option<String>, AppError> {
    match present(fields.get(key)) {
        None => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(Value::Number(number)) => Ok(Some(number.to_string())),
        Some(Value::Bool(flag)) => Ok(Some(flag.to_string())),
        Some(_) => Err(invalid(label, &format!("{prefix}.{key}"), "must be text.")),
    }
}

pub(super) fn req_text(
    label: &str,
    fields: &Fields,
    prefix: &str,
    key: &str,
) -> Result<String, AppError> {
    opt_text(label, fields, prefix, key)?
        .ok_or_else(|| invalid(label, &format!("{prefix}.{key}"), "is missing."))
}

pub(super) fn text_or_empty(
    label: &str,
    fields: &Fields,
    prefix: &str,
    key: &str,
) -> Result<String, AppError> {
    Ok(opt_text(label, fields, prefix, key)?.unwrap_or_default())
}

fn opt_bool(
    label: &str,
    fields: &Fields,
    prefix: &str,
    key: &str,
) -> Result<Option<bool>, AppError> {
    match present(fields.get(key)) {
        None => Ok(None),
        Some(Value::Bool(flag)) => Ok(Some(*flag)),
        Some(_) => Err(invalid(
            label,
            &format!("{prefix}.{key}"),
            "must be true or false.",
        )),
    }
}

fn non_empty(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.is_empty())
}

fn parse_key_value(label: &str, fields: &Fields, list: &str) -> Result<KeyValue, AppError> {
    Ok(KeyValue {
        name: req_text(label, fields, list, "name")?,
        value: text_or_empty(label, fields, list, "value")?,
        enabled: opt_bool(label, fields, list, "enabled")?.unwrap_or(true),
    })
}

fn parse_pair(label: &str, fields: &Fields, list: &str) -> Result<Pair, AppError> {
    Ok(Pair {
        name: req_text(label, fields, list, "name")?,
        value: text_or_empty(label, fields, list, "value")?,
    })
}

fn parse_part(label: &str, fields: &Fields, list: &str) -> Result<Part, AppError> {
    Ok(Part {
        name: req_text(label, fields, list, "name")?,
        value: text_or_empty(label, fields, list, "value")?,
        file: non_empty(opt_text(label, fields, list, "file")?),
        enabled: opt_bool(label, fields, list, "enabled")?.unwrap_or(true),
    })
}

fn parse_body(label: &str, value: Option<&Value>) -> Result<Body, AppError> {
    let Some(value) = present(value) else {
        return Ok(Body::None);
    };
    let fields = as_map(label, value, BODY)?;
    let text = || req_text(label, fields, BODY, "text");
    Ok(match req_text(label, fields, BODY, "type")?.as_str() {
        "none" => Body::None,
        "json" => Body::Json { text: text()? },
        "xml" => Body::Xml { text: text()? },
        "text" => Body::Text { text: text()? },
        "form" => Body::Form {
            fields: parse_list(label, fields.get("fields"), "body.fields", parse_key_value)?,
        },
        "multipart" => Body::Multipart {
            parts: parse_list(label, fields.get("parts"), "body.parts", parse_part)?,
        },
        "binary" => Body::Binary {
            file: non_empty(opt_text(label, fields, BODY, "file")?),
        },
        "graphql" => protocol_model::parse_graphql(label, fields)?,
        _ => Body::Unsupported,
    })
}

fn parse_settings(label: &str, value: Option<&Value>) -> Result<Settings, AppError> {
    let Some(value) = present(value) else {
        return Ok(Settings::default());
    };
    let fields = as_map(label, value, SETTINGS)?;
    let timeout_ms = match present(fields.get("timeout_ms")) {
        None => DEFAULT_TIMEOUT_MS,
        Some(value) => value
            .as_u64()
            .and_then(|ms| u32::try_from(ms).ok())
            .ok_or_else(|| timeout_error(label))?,
    };
    Ok(Settings {
        follow_redirects: opt_bool(label, fields, SETTINGS, "follow_redirects")?.unwrap_or(true),
        timeout_ms,
        verify_tls: opt_bool(label, fields, SETTINGS, "verify_tls")?.unwrap_or(true),
        http_version: parse_http_version(label, fields)?,
    })
}

fn parse_http_version(label: &str, fields: &Fields) -> Result<HttpVersion, AppError> {
    match opt_text(label, fields, SETTINGS, "http_version")?.as_deref() {
        None | Some("auto") => Ok(HttpVersion::Auto),
        Some("http1") => Ok(HttpVersion::Http1),
        Some("http2") => Ok(HttpVersion::Http2),
        Some(_) => Err(invalid(
            label,
            "settings.http_version",
            "must be `auto`, `http1` or `http2`.",
        )),
    }
}

fn timeout_error(label: &str) -> AppError {
    invalid(
        label,
        "settings.timeout_ms",
        "must be a whole number from 0 to 600000.",
    )
}

fn validate_sections(label: &str, content: &RequestContent) -> Result<(), AppError> {
    let pairs = content.disabled_params.iter();
    check_entries(
        label,
        DISABLED_PARAMS,
        pairs.map(|p| (&p.name, &p.value)),
        MAX_ENTRIES,
    )?;
    let headers = content.headers.iter();
    check_entries(
        label,
        HEADERS,
        headers.map(|h| (&h.name, &h.value)),
        MAX_ENTRIES,
    )?;
    validate_body(label, &content.body)?;
    protocol_model::validate(label, content)?;
    validate_auth(label, &content.auth)?;
    if content.settings.timeout_ms > MAX_TIMEOUT_MS {
        return Err(timeout_error(label));
    }
    validate_scripts(label, &content.scripts)
}

fn validate_body(label: &str, body: &Body) -> Result<(), AppError> {
    match body {
        Body::Json { text } | Body::Xml { text } | Body::Text { text } => {
            if text.len() > MAX_BODY_TEXT_BYTES {
                return Err(invalid(label, "body.text", "is larger than 4 MiB."));
            }
            Ok(())
        }
        Body::Form { fields } => {
            let entries = fields.iter().map(|f| (&f.name, &f.value));
            check_entries(label, "body.fields", entries, MAX_ENTRIES)
        }
        Body::Multipart { parts } => {
            let entries = parts.iter().map(|p| (&p.name, &p.value));
            check_entries(label, "body.parts", entries, MAX_PARTS)?;
            parts
                .iter()
                .filter_map(|part| part.file.as_deref())
                .try_for_each(|file| {
                    check_text(label, "body.parts.file", file, MAX_FILE_PATH_CHARS)
                })
        }
        Body::Binary { file } => file.as_deref().map_or(Ok(()), |file| {
            check_text(label, "body.file", file, MAX_FILE_PATH_CHARS)
        }),
        // GraphQL limits are checked by `protocol_model::validate`.
        Body::None | Body::Graphql { .. } | Body::Unsupported => Ok(()),
    }
}

fn check_entries<'a>(
    label: &str,
    field: &str,
    entries: impl ExactSizeIterator<Item = (&'a String, &'a String)>,
    max: usize,
) -> Result<(), AppError> {
    if entries.len() > max {
        return Err(invalid(
            label,
            field,
            &format!("has more than {max} entries."),
        ));
    }
    let (name_field, value_field) = (format!("{field}.name"), format!("{field}.value"));
    entries.into_iter().try_for_each(|(name, value)| {
        check_text(label, &name_field, name, MAX_ENTRY_NAME_CHARS)?;
        check_text(label, &value_field, value, MAX_ENTRY_VALUE_CHARS)
    })
}

pub(super) fn check_text(
    label: &str,
    field: &str,
    text: &str,
    max_chars: usize,
) -> Result<(), AppError> {
    if text.chars().count() > max_chars {
        return Err(invalid(
            label,
            field,
            &format!("is longer than {max_chars} characters."),
        ));
    }
    if text.chars().any(char::is_control) {
        return Err(invalid(
            label,
            field,
            "must not contain control characters.",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "request_model_tests.rs"]
mod tests;
