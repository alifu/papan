//! The `scripts` section (plan 05 §3), shared by request files, `_folder.yaml` and
//! `papan.collection.yaml`: parsing, validation and canonical emit. Empty scripts are
//! omitted; unknown keys inside the section are kept.

use serde_json::Value;

use super::request_emit::overlay;
use super::request_model::{as_map, invalid, present, text_or_empty, Extra, Fields};
use super::types::Scripts;
use crate::error::AppError;

pub const SCRIPTS: &str = "scripts";
pub const MAX_SCRIPT_BYTES: usize = 256 * 1024;
const PRE_REQUEST: &str = "pre_request";
const POST_RESPONSE: &str = "post_response";
const SCRIPT_KEYS: [&str; 2] = [POST_RESPONSE, PRE_REQUEST];

/// The typed `scripts` section, validated; missing or null is empty.
pub fn parse_scripts(label: &str, value: Option<&Value>) -> Result<Scripts, AppError> {
    let Some(value) = present(value) else {
        return Ok(Scripts::default());
    };
    let fields = as_map(label, value, SCRIPTS)?;
    let scripts = Scripts {
        pre_request: script_text(label, fields, PRE_REQUEST)?,
        post_response: script_text(label, fields, POST_RESPONSE)?,
    };
    validate_scripts(label, &scripts)?;
    Ok(scripts)
}

/// Text only: a script that YAML reads as a number or flag is still rejected.
fn script_text(label: &str, fields: &Fields, key: &str) -> Result<String, AppError> {
    match present(fields.get(key)) {
        None | Some(Value::String(_)) => text_or_empty(label, fields, SCRIPTS, key),
        Some(_) => Err(invalid(label, &format!("{SCRIPTS}.{key}"), "must be text.")),
    }
}

pub fn validate_scripts(label: &str, scripts: &Scripts) -> Result<(), AppError> {
    [
        (PRE_REQUEST, &scripts.pre_request),
        (POST_RESPONSE, &scripts.post_response),
    ]
    .into_iter()
    .try_for_each(|(key, text)| {
        if text.len() > MAX_SCRIPT_BYTES {
            return Err(invalid(
                label,
                &format!("{SCRIPTS}.{key}"),
                "is larger than 256 KiB.",
            ));
        }
        Ok(())
    })
}

/// The canonical section, or `None` when both scripts are empty and `raw` holds nothing
/// else worth keeping.
pub fn emit_scripts(scripts: &Scripts, raw: Option<&Value>) -> Option<Value> {
    let canonical: Fields = [
        (PRE_REQUEST, &scripts.pre_request),
        (POST_RESPONSE, &scripts.post_response),
    ]
    .into_iter()
    .filter(|(_, text)| !text.is_empty())
    .map(|(key, text)| (key.to_string(), Value::String(text.clone())))
    .collect();
    let all = overlay(canonical, raw, &SCRIPT_KEYS);
    (!all.is_empty()).then_some(Value::Object(all))
}

/// `extra` with its `scripts` section set to `scripts` (removed when empty).
pub fn with_scripts(extra: &Extra, scripts: &Scripts) -> Extra {
    let section = emit_scripts(scripts, extra.get(SCRIPTS));
    extra
        .iter()
        .filter(|(key, _)| key.as_str() != SCRIPTS)
        .map(|(key, value)| (key.clone(), value.clone()))
        .chain(section.map(|value| (SCRIPTS.to_string(), value)))
        .collect()
}

#[cfg(test)]
#[path = "script_model_tests.rs"]
mod tests;
