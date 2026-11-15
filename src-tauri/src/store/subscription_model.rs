//! The `subscription` section of a `kind: graphql` request (plan 06b §3): the WebSocket
//! URL override and the `connection_init` payload. Called from `request_model`,
//! `request_emit` and `protocol_model`.

use serde_json::Value;

use super::request_emit::overlay;
use super::request_model::{as_map, check_text, invalid, present, text_or_empty};
use super::types::{RequestContent, RequestKind, SubscriptionSettings};
use crate::error::AppError;

pub const SUBSCRIPTION: &str = "subscription";
pub const MAX_URL_CHARS: usize = 8192;
pub const MAX_INIT_PAYLOAD_BYTES: usize = 64 * 1024;
const KEYS: [&str; 2] = ["init_payload", "url"];
const VARIABLE_OPEN: &str = "{{";

/// The `subscription` section; present only in `kind: graphql`.
pub fn parse_subscription(
    label: &str,
    kind: RequestKind,
    value: Option<&Value>,
) -> Result<SubscriptionSettings, AppError> {
    let Some(value) = present(value) else {
        return Ok(SubscriptionSettings::default());
    };
    if kind != RequestKind::Graphql {
        return Err(outside(label));
    }
    let fields = as_map(label, value, SUBSCRIPTION)?;
    Ok(SubscriptionSettings {
        url: text_or_empty(label, fields, SUBSCRIPTION, "url")?,
        init_payload: text_or_empty(label, fields, SUBSCRIPTION, "init_payload")?,
    })
}

/// Limits of the section and the kind rule.
pub fn validate(label: &str, content: &RequestContent) -> Result<(), AppError> {
    let settings = &content.subscription;
    if content.kind != RequestKind::Graphql {
        if *settings != SubscriptionSettings::default() {
            return Err(outside(label));
        }
        return Ok(());
    }
    check_text(label, "subscription.url", &settings.url, MAX_URL_CHARS)?;
    if !settings.url.is_empty() && !valid_url_start(&settings.url) {
        return Err(invalid(
            label,
            "subscription.url",
            "must start with `ws://`, `wss://` or a `{{variable}}`.",
        ));
    }
    if settings.init_payload.len() > MAX_INIT_PAYLOAD_BYTES {
        return Err(invalid(
            label,
            "subscription.init_payload",
            "is larger than 64 KiB.",
        ));
    }
    Ok(())
}

fn valid_url_start(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    lower.starts_with("ws://") || lower.starts_with("wss://") || lower.starts_with(VARIABLE_OPEN)
}

fn outside(label: &str) -> AppError {
    invalid(label, SUBSCRIPTION, "is only allowed in `kind: graphql`.")
}

/// The canonical section; `None` when all default. Unknown keys of `raw` are kept.
pub fn emit_subscription(settings: &SubscriptionSettings, raw: Option<&Value>) -> Option<Value> {
    let text = |value: &str| (!value.is_empty()).then(|| Value::String(value.to_string()));
    let canonical = [
        ("init_payload", text(&settings.init_payload)),
        ("url", text(&settings.url)),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.map(|value| (key.to_string(), value)))
    .collect();
    let all = overlay(canonical, raw, &KEYS);
    (!all.is_empty()).then_some(Value::Object(all))
}

#[cfg(test)]
#[path = "subscription_model_tests.rs"]
mod tests;
