//! The `grpc` section of a `kind: grpc` request (plan 08 §3): parsing, the kind and
//! metadata rules, and the canonical emit. Called from `request_model`, `request_emit` and
//! `protocol_model`. Empty `import_paths` means the default `[.]`; `source: protos` needing
//! at least one proto is checked when the schema is built, so a new request can be saved.

use base64::alphabet::STANDARD as STANDARD_ALPHABET;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use base64::Engine as _;
use serde_json::Value;

use super::paths;
use super::request_emit::overlay;
use super::request_model::{as_map, check_text, invalid, present, text_or_empty};
use super::types::{GrpcSettings, GrpcSource, KeyValue, RequestContent, RequestKind, Scripts};
use crate::error::AppError;

pub const GRPC: &str = "grpc";
pub const MAX_PROTOS: usize = 50;
pub const MAX_IMPORT_PATHS: usize = 20;
pub const MAX_PATH_CHARS: usize = 512;
pub const MAX_NAME_CHARS: usize = 256;
pub const MAX_METHOD_CHARS: usize = 128;
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
pub const MAX_DEADLINE_MS: u32 = 3_600_000;
pub const MAX_METADATA: usize = 100;
const KEYS: [&str; 7] = [
    "deadline_ms",
    "import_paths",
    "message",
    "method",
    "protos",
    "service",
    "source",
];
const GRPC_METHOD: &str = "POST";
const PROTO_SUFFIX: &str = ".proto";
const CURRENT_DIR: &str = ".";
const VARIABLE_OPEN: &str = "{{";
const BIN_SUFFIX: &str = "-bin";
const RESERVED_NAMES: [&str; 4] = ["content-type", "te", "host", "connection"];
const BIN_BASE64: GeneralPurpose = GeneralPurpose::new(
    &STANDARD_ALPHABET,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// The `grpc` section; present only in `kind: grpc`.
pub fn parse_grpc(
    label: &str,
    kind: RequestKind,
    value: Option<&Value>,
) -> Result<GrpcSettings, AppError> {
    let Some(value) = present(value) else {
        return Ok(GrpcSettings::default());
    };
    if kind != RequestKind::Grpc {
        return Err(outside(label));
    }
    let fields = as_map(label, value, GRPC)?;
    Ok(GrpcSettings {
        source: parse_source(label, fields.get("source"))?,
        protos: parse_texts(label, fields.get("protos"), "grpc.protos")?,
        import_paths: parse_texts(label, fields.get("import_paths"), "grpc.import_paths")?,
        service: text_or_empty(label, fields, GRPC, "service")?,
        method: text_or_empty(label, fields, GRPC, "method")?,
        message: text_or_empty(label, fields, GRPC, "message")?,
        deadline_ms: parse_deadline(label, fields.get("deadline_ms"))?,
    })
}

fn parse_source(label: &str, value: Option<&Value>) -> Result<GrpcSource, AppError> {
    match present(value).map(Value::as_str) {
        None | Some(Some("protos")) => Ok(GrpcSource::Protos),
        Some(Some("reflection")) => Ok(GrpcSource::Reflection),
        Some(_) => Err(invalid(
            label,
            "grpc.source",
            "must be `protos` or `reflection`.",
        )),
    }
}

fn parse_texts(label: &str, value: Option<&Value>, field: &str) -> Result<Vec<String>, AppError> {
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

fn parse_deadline(label: &str, value: Option<&Value>) -> Result<u32, AppError> {
    let Some(value) = present(value) else {
        return Ok(0);
    };
    value
        .as_u64()
        .and_then(|ms| u32::try_from(ms).ok())
        .ok_or_else(|| invalid(label, "grpc.deadline_ms", "must be a whole number of ms."))
}

/// The kind consistency rules, the limits of the section and the metadata rules.
pub fn validate(label: &str, content: &RequestContent) -> Result<(), AppError> {
    if content.kind != RequestKind::Grpc {
        if content.grpc != GrpcSettings::default() {
            return Err(outside(label));
        }
        return Ok(());
    }
    check_kind_sections(label, content)?;
    validate_settings(label, &content.grpc)?;
    validate_metadata(label, &content.headers)
}

fn check_kind_sections(label: &str, content: &RequestContent) -> Result<(), AppError> {
    let not_allowed = [
        ("scripts", content.scripts != Scripts::default()),
        ("soap", content.soap != Default::default()),
        ("ws", content.ws != Default::default()),
        ("subscription", content.subscription != Default::default()),
    ];
    match not_allowed.into_iter().find(|(_, present)| *present) {
        Some((section, _)) => Err(invalid(label, section, "is not allowed in `kind: grpc`.")),
        None => Ok(()),
    }
}

/// `POST` is the only method of a gRPC request.
pub fn check_method(label: &str, method: &str) -> Result<(), AppError> {
    if method != GRPC_METHOD {
        return Err(invalid(
            label,
            "method",
            "must be `POST` in a gRPC request.",
        ));
    }
    Ok(())
}

/// `http`/`https` and a host, no path beyond `/`, no query; an empty URL or one starting
/// with a `{{variable}}` is checked after resolution.
pub fn check_url(label: &str, url: &str) -> Result<(), AppError> {
    let lower = url.trim().to_ascii_lowercase();
    if lower.is_empty() || lower.starts_with(VARIABLE_OPEN) {
        return Ok(());
    }
    let rest = lower
        .strip_prefix("http://")
        .or_else(|| lower.strip_prefix("https://"));
    let ok = rest.is_some_and(|rest| {
        let (_, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
        let no_extras = !rest.contains(['?', '#']);
        no_extras && (path.is_empty() || path == "/")
    });
    if ok {
        return Ok(());
    }
    Err(invalid(
        label,
        "url",
        "must be `http://` or `https://` and a host, with no path or query.",
    ))
}

fn validate_settings(label: &str, grpc: &GrpcSettings) -> Result<(), AppError> {
    check_dotted_name(label, "grpc.service", &grpc.service)?;
    check_name(label, "grpc.method", &grpc.method, MAX_METHOD_CHARS, false)?;
    check_count(label, "grpc.protos", grpc.protos.len(), MAX_PROTOS)?;
    check_count(
        label,
        "grpc.import_paths",
        grpc.import_paths.len(),
        MAX_IMPORT_PATHS,
    )?;
    for proto in &grpc.protos {
        check_rel(label, "grpc.protos", proto, true)?;
    }
    for dir in &grpc.import_paths {
        check_rel(label, "grpc.import_paths", dir, false)?;
    }
    if grpc.message.len() > MAX_MESSAGE_BYTES {
        return Err(invalid(label, "grpc.message", "is larger than 1 MiB."));
    }
    if grpc.deadline_ms > MAX_DEADLINE_MS {
        return Err(invalid(
            label,
            "grpc.deadline_ms",
            "must be at most 3600000.",
        ));
    }
    Ok(())
}

fn check_count(label: &str, field: &str, count: usize, max: usize) -> Result<(), AppError> {
    if count > max {
        return Err(invalid(
            label,
            field,
            &format!("has more than {max} entries."),
        ));
    }
    Ok(())
}

fn check_dotted_name(label: &str, field: &str, name: &str) -> Result<(), AppError> {
    check_name(label, field, name, MAX_NAME_CHARS, true)
}

/// An identifier (`[A-Za-z_][A-Za-z0-9_]*`), or dot-separated ones; empty is allowed.
fn check_name(
    label: &str,
    field: &str,
    name: &str,
    max_chars: usize,
    dotted: bool,
) -> Result<(), AppError> {
    let identifier = |part: &str| {
        let mut chars = part.chars();
        let first = chars
            .next()
            .is_some_and(|c| c == '_' || c.is_ascii_alphabetic());
        first && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
    };
    let ok = name.is_empty()
        || (name.chars().count() <= max_chars
            && if dotted {
                name.split('.').all(identifier)
            } else {
                identifier(name)
            });
    if ok {
        return Ok(());
    }
    let what = if dotted {
        "a dotted identifier"
    } else {
        "an identifier"
    };
    Err(invalid(
        label,
        field,
        &format!("must be {what} of at most {max_chars} characters."),
    ))
}

/// A path relative to the protos root: checked segments, `.proto` for files, `.` for the
/// root itself among the import paths.
fn check_rel(label: &str, field: &str, rel: &str, file: bool) -> Result<(), AppError> {
    check_text(label, field, rel, MAX_PATH_CHARS)?;
    let root = !file && rel == CURRENT_DIR;
    let ok = root
        || (!rel.is_empty()
            && paths::segments(rel).is_ok()
            && (!file || rel.ends_with(PROTO_SUFFIX)));
    if ok {
        return Ok(());
    }
    let problem = if file {
        "must be a relative path to a `.proto` file inside the protos folder."
    } else {
        "must be a relative folder inside the protos folder."
    };
    Err(invalid(label, field, problem))
}

fn validate_metadata(label: &str, rows: &[KeyValue]) -> Result<(), AppError> {
    check_count(label, "headers", rows.len(), MAX_METADATA)?;
    rows.iter().try_for_each(|row| {
        check_metadata_name(label, &row.name)?;
        let bin = row.name.ends_with(BIN_SUFFIX);
        if bin && !row.value.contains(VARIABLE_OPEN) && !is_valid_bin_value(&row.value) {
            return Err(invalid(
                label,
                "headers.value",
                "must be base64 in a `-bin` metadata entry.",
            ));
        }
        Ok(())
    })
}

fn check_metadata_name(label: &str, name: &str) -> Result<(), AppError> {
    if name.contains(VARIABLE_OPEN) {
        return Ok(());
    }
    let lower = name.to_ascii_lowercase();
    let reserved = lower.starts_with(':')
        || lower.starts_with("grpc-")
        || RESERVED_NAMES.contains(&lower.as_str());
    if reserved {
        return Err(invalid(
            label,
            "headers.name",
            "is reserved by gRPC and cannot be metadata.",
        ));
    }
    if name.chars().any(|c| c.is_ascii_uppercase()) {
        return Err(invalid(
            label,
            "headers.name",
            "must be lowercase in gRPC metadata.",
        ));
    }
    Ok(())
}

/// Whether `value` decodes as base64 (padding optional), as `-bin` metadata needs.
pub fn is_valid_bin_value(value: &str) -> bool {
    BIN_BASE64.decode(value.trim()).is_ok()
}

fn outside(label: &str) -> AppError {
    invalid(label, GRPC, "is only allowed in `kind: grpc`.")
}

fn texts(items: &[String]) -> Option<Value> {
    (!items.is_empty()).then(|| Value::Array(items.iter().cloned().map(Value::String).collect()))
}

fn text(value: &str) -> Option<Value> {
    (!value.is_empty()).then(|| Value::String(value.to_string()))
}

/// The canonical `grpc` section; `None` when all default. Unknown keys of `raw` are kept.
pub fn emit_grpc(grpc: &GrpcSettings, raw: Option<&Value>) -> Option<Value> {
    let source =
        (grpc.source == GrpcSource::Reflection).then(|| Value::String("reflection".into()));
    let deadline = (grpc.deadline_ms > 0).then(|| Value::from(grpc.deadline_ms));
    let canonical = [
        ("deadline_ms", deadline),
        ("import_paths", texts(&grpc.import_paths)),
        ("message", text(&grpc.message)),
        ("method", text(&grpc.method)),
        ("protos", texts(&grpc.protos)),
        ("service", text(&grpc.service)),
        ("source", source),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.map(|value| (key.to_string(), value)))
    .collect();
    let all = overlay(canonical, raw, &KEYS);
    (!all.is_empty()).then_some(Value::Object(all))
}

#[cfg(test)]
#[path = "grpc_model_tests.rs"]
mod tests;
