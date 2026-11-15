//! The `ws` section of a `kind: ws` request (plan 07 §3): subprotocols and saved message
//! templates, with the consistency rules of a WebSocket request. Called from
//! `request_model`, `request_emit` and `protocol_model`.

use std::collections::HashSet;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde_json::Value;

use super::request_emit::overlay;
use super::request_model::{as_map, check_text, invalid, present, text_or_empty, Fields};
use super::types::{RequestContent, RequestKind, Scripts, WsFormat, WsSettings, WsTemplate};
use crate::error::AppError;

pub const WS: &str = "ws";
pub const MAX_SUBPROTOCOLS: usize = 20;
pub const MAX_SUBPROTOCOL_CHARS: usize = 128;
pub const MAX_TEMPLATES: usize = 50;
pub const MAX_TEMPLATE_NAME_CHARS: usize = 256;
pub const MAX_TEMPLATE_BYTES: usize = 1024 * 1024;
const WS_KEYS: [&str; 2] = ["messages", "subprotocols"];
const TEMPLATE_KEYS: [&str; 3] = ["format", "name", "text"];
const WS_METHOD: &str = "GET";
const VARIABLE_OPEN: &str = "{{";

/// The `ws` section; present only in `kind: ws`.
pub fn parse_ws(
    label: &str,
    kind: RequestKind,
    value: Option<&Value>,
) -> Result<WsSettings, AppError> {
    let Some(value) = present(value) else {
        return Ok(WsSettings::default());
    };
    if kind != RequestKind::Ws {
        return Err(outside(label));
    }
    let fields = as_map(label, value, WS)?;
    Ok(WsSettings {
        subprotocols: parse_subprotocols(label, fields.get("subprotocols"))?,
        messages: parse_templates(label, fields.get("messages"))?,
    })
}

fn parse_subprotocols(label: &str, value: Option<&Value>) -> Result<Vec<String>, AppError> {
    let field = "ws.subprotocols";
    let Some(value) = present(value) else {
        return Ok(Vec::new());
    };
    let items = value
        .as_array()
        .ok_or_else(|| invalid(label, field, "must be a list."))?;
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| invalid(label, field, "must hold text."))
        })
        .collect()
}

fn parse_templates(label: &str, value: Option<&Value>) -> Result<Vec<WsTemplate>, AppError> {
    let field = "ws.messages";
    let Some(value) = present(value) else {
        return Ok(Vec::new());
    };
    let items = value
        .as_array()
        .ok_or_else(|| invalid(label, field, "must be a list."))?;
    items
        .iter()
        .map(|item| parse_template(label, as_map(label, item, field)?))
        .collect()
}

fn parse_template(label: &str, fields: &Fields) -> Result<WsTemplate, AppError> {
    let field = "ws.messages";
    let name = match present(fields.get("name")) {
        Some(Value::String(name)) => name.clone(),
        _ => return Err(invalid(label, "ws.messages.name", "is missing.")),
    };
    let format = match present(fields.get("format")).map(Value::as_str) {
        None | Some(Some("text")) => WsFormat::Text,
        Some(Some("json")) => WsFormat::Json,
        Some(Some("binary")) => WsFormat::Binary,
        Some(_) => {
            return Err(invalid(
                label,
                "ws.messages.format",
                "must be `text`, `json` or `binary`.",
            ))
        }
    };
    Ok(WsTemplate {
        name,
        format,
        text: text_or_empty(label, fields, field, "text")?,
    })
}

/// Limits of the `ws` section and the kind consistency rules (plan 07 §3).
pub fn validate(label: &str, content: &RequestContent) -> Result<(), AppError> {
    if content.kind != RequestKind::Ws {
        if content.ws != WsSettings::default() {
            return Err(outside(label));
        }
        return Ok(());
    }
    if content.scripts != Scripts::default() {
        return Err(invalid(label, "scripts", "is not allowed in `kind: ws`."));
    }
    if content.soap != Default::default() {
        return Err(invalid(label, "soap", "is only allowed in `kind: soap`."));
    }
    validate_subprotocols(label, &content.ws.subprotocols)?;
    validate_templates(label, &content.ws.messages)
}

/// `GET` is the only method of a WebSocket request.
pub fn check_method(label: &str, method: &str) -> Result<(), AppError> {
    if method != WS_METHOD {
        return Err(invalid(
            label,
            "method",
            "must be `GET` in a WebSocket request.",
        ));
    }
    Ok(())
}

/// The scheme is `ws` or `wss`; an empty URL or one starting with a `{{variable}}` is
/// checked after resolution.
pub fn check_url(label: &str, url: &str) -> Result<(), AppError> {
    let lower = url.trim().to_ascii_lowercase();
    let ok = lower.is_empty()
        || lower.starts_with(VARIABLE_OPEN)
        || lower.starts_with("ws://")
        || lower.starts_with("wss://");
    if ok {
        return Ok(());
    }
    Err(invalid(
        label,
        "url",
        "must start with `ws://` or `wss://`.",
    ))
}

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c)
}

fn validate_subprotocols(label: &str, protocols: &[String]) -> Result<(), AppError> {
    let field = "ws.subprotocols";
    if protocols.len() > MAX_SUBPROTOCOLS {
        return Err(invalid(
            label,
            field,
            &format!("has more than {MAX_SUBPROTOCOLS} entries."),
        ));
    }
    let mut seen = HashSet::new();
    for protocol in protocols {
        let long = protocol.chars().count() > MAX_SUBPROTOCOL_CHARS;
        if protocol.is_empty() || long || !protocol.chars().all(is_token_char) {
            return Err(invalid(
                label,
                field,
                "must hold tokens of 1 to 128 characters (letters, digits, `-._~!#$%&'*+^`|`).",
            ));
        }
        if !seen.insert(protocol.as_str()) {
            return Err(invalid(label, field, "has a duplicate entry."));
        }
    }
    Ok(())
}

fn validate_templates(label: &str, templates: &[WsTemplate]) -> Result<(), AppError> {
    if templates.len() > MAX_TEMPLATES {
        return Err(invalid(
            label,
            "ws.messages",
            &format!("has more than {MAX_TEMPLATES} entries."),
        ));
    }
    let mut seen = HashSet::new();
    for template in templates {
        check_text(
            label,
            "ws.messages.name",
            &template.name,
            MAX_TEMPLATE_NAME_CHARS,
        )?;
        if template.name.is_empty() || !seen.insert(template.name.as_str()) {
            return Err(invalid(
                label,
                "ws.messages.name",
                "must be non-empty and unique.",
            ));
        }
        validate_template_text(label, template)?;
    }
    Ok(())
}

fn validate_template_text(label: &str, template: &WsTemplate) -> Result<(), AppError> {
    let field = "ws.messages.text";
    if template.text.len() > MAX_TEMPLATE_BYTES {
        return Err(invalid(label, field, "is larger than 1 MiB."));
    }
    // A `{{variable}}` is decoded only after resolution.
    let has_variable = template.text.contains(VARIABLE_OPEN);
    let bad_base64 = template.format == WsFormat::Binary
        && !has_variable
        && STANDARD.decode(template.text.trim()).is_err();
    if bad_base64 {
        return Err(invalid(
            label,
            field,
            "must be valid base64 in a binary message.",
        ));
    }
    Ok(())
}

fn outside(label: &str) -> AppError {
    invalid(label, WS, "is only allowed in `kind: ws`.")
}

fn text(value: &str) -> Option<Value> {
    (!value.is_empty()).then(|| Value::String(value.to_string()))
}

fn format_name(format: WsFormat) -> Option<Value> {
    match format {
        WsFormat::Text => None,
        WsFormat::Json => Some(Value::String("json".into())),
        WsFormat::Binary => Some(Value::String("binary".into())),
    }
}

fn template_value(template: &WsTemplate) -> Value {
    let entries = [
        ("format", format_name(template.format)),
        ("name", Some(Value::String(template.name.clone()))),
        ("text", text(&template.text)),
    ];
    let canonical = entries
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key.to_string(), value)))
        .collect();
    Value::Object(overlay(canonical, None, &TEMPLATE_KEYS))
}

/// The canonical `ws` section; `None` when all default. Unknown keys of `raw` are kept.
pub fn emit_ws(ws: &WsSettings, raw: Option<&Value>) -> Option<Value> {
    let subprotocols = (!ws.subprotocols.is_empty()).then(|| {
        Value::Array(
            ws.subprotocols
                .iter()
                .map(|p| Value::String(p.clone()))
                .collect(),
        )
    });
    let messages = (!ws.messages.is_empty())
        .then(|| Value::Array(ws.messages.iter().map(template_value).collect()));
    let canonical = [("messages", messages), ("subprotocols", subprotocols)]
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key.to_string(), value)))
        .collect();
    let all = overlay(canonical, raw, &WS_KEYS);
    (!all.is_empty()).then_some(Value::Object(all))
}

#[cfg(test)]
#[path = "ws_model_tests.rs"]
mod tests;
