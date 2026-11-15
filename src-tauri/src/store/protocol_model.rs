//! The protocol parts of a request file (plan 06 §3): `kind`, the `graphql` body, the
//! `soap` section and the consistency rules between them. `request_model` and
//! `request_emit` call in here; the generic section machinery stays there.

use serde_json::Value;

use super::model::RequestFile;
use super::request_emit::overlay;
use super::request_model::{
    as_map, check_text, invalid, opt_text, present, text_or_empty, Fields, BODY,
};
use super::subscription_model;
use super::types::{Body, RequestContent, RequestKind, SoapSettings, SoapVersion, WsdlRef};
use super::ws_model;
use super::yaml;
use crate::error::AppError;

pub const SOAP: &str = "soap";
pub const MAX_GRAPHQL_TEXT_BYTES: usize = 1024 * 1024;
const MAX_OPERATION_NAME_CHARS: usize = 256;
const MAX_ACTION_CHARS: usize = 2048;
const MAX_WSDL_FIELD_CHARS: usize = 256;
const MAX_WSDL_URL_CHARS: usize = 8192;
const GRAPHQL_METHOD: &str = "POST";
const GRAPHQL_KEYS: [&str; 3] = ["operation_name", "query", "variables"];
const SOAP_KEYS: [&str; 3] = ["action", "version", "wsdl"];
const WSDL_KEYS: [&str; 5] = ["file", "operation", "port", "service", "url"];

pub fn kind_name(kind: RequestKind) -> &'static str {
    match kind {
        RequestKind::Http => "http",
        RequestKind::Graphql => "graphql",
        RequestKind::Soap => "soap",
        RequestKind::Ws => "ws",
    }
}

/// The on-disk `kind`; a newer kind (`grpc`) is invalid.
pub fn parse_kind(label: &str, kind: &str) -> Result<RequestKind, AppError> {
    match kind {
        "http" => Ok(RequestKind::Http),
        "graphql" => Ok(RequestKind::Graphql),
        "soap" => Ok(RequestKind::Soap),
        "ws" => Ok(RequestKind::Ws),
        _ => Err(yaml::invalid(
            label,
            "field `kind` must be `http`, `graphql`, `soap` or `ws`.",
        )),
    }
}

/// The content a new request of `kind` starts with (plan 06 §8).
pub fn starter_content(kind: RequestKind) -> RequestContent {
    match kind {
        RequestKind::Http => RequestContent::new("GET", ""),
        RequestKind::Graphql => RequestContent {
            body: empty_graphql(),
            kind,
            ..RequestContent::new(GRAPHQL_METHOD, "")
        },
        RequestKind::Soap => RequestContent {
            body: Body::Xml {
                text: String::new(),
            },
            kind,
            ..RequestContent::new(GRAPHQL_METHOD, "")
        },
        RequestKind::Ws => RequestContent {
            kind,
            ..RequestContent::new("GET", "")
        },
    }
}

/// A new request file of `kind` holding [`starter_content`].
pub fn new_request_file(name: String, kind: RequestKind) -> Result<RequestFile, AppError> {
    let content = starter_content(kind);
    RequestFile::new(name, &content.method, kind).with_content(&content)
}

fn empty_graphql() -> Body {
    Body::Graphql {
        query: String::new(),
        variables: String::new(),
        operation_name: String::new(),
    }
}

/// A missing body of a GraphQL request reads as an empty GraphQL body.
pub fn body_for_kind(kind: RequestKind, body: Body) -> Body {
    match (kind, body) {
        (RequestKind::Graphql, Body::None) => empty_graphql(),
        (_, body) => body,
    }
}

pub fn parse_graphql(label: &str, fields: &Fields) -> Result<Body, AppError> {
    Ok(Body::Graphql {
        query: text_or_empty(label, fields, BODY, "query")?,
        variables: text_or_empty(label, fields, BODY, "variables")?,
        operation_name: text_or_empty(label, fields, BODY, "operation_name")?,
    })
}

/// The `soap` section; present only in `kind: soap`.
pub fn parse_soap(
    label: &str,
    kind: RequestKind,
    value: Option<&Value>,
) -> Result<SoapSettings, AppError> {
    let Some(value) = present(value) else {
        return Ok(SoapSettings::default());
    };
    if kind != RequestKind::Soap {
        return Err(soap_outside(label));
    }
    let fields = as_map(label, value, SOAP)?;
    Ok(SoapSettings {
        version: parse_version(label, fields)?,
        action: text_or_empty(label, fields, SOAP, "action")?,
        wsdl: parse_wsdl(label, fields.get("wsdl"))?,
    })
}

fn parse_version(label: &str, fields: &Fields) -> Result<SoapVersion, AppError> {
    let version = match present(fields.get("version")) {
        Some(Value::Number(number)) => Some(number.to_string()),
        _ => opt_text(label, fields, SOAP, "version")?,
    };
    match version.as_deref() {
        None | Some("1.1") => Ok(SoapVersion::V11),
        Some("1.2") => Ok(SoapVersion::V12),
        Some(_) => Err(invalid(
            label,
            "soap.version",
            "must be \"1.1\" or \"1.2\".",
        )),
    }
}

fn parse_wsdl(label: &str, value: Option<&Value>) -> Result<Option<WsdlRef>, AppError> {
    let Some(value) = present(value) else {
        return Ok(None);
    };
    let fields = as_map(label, value, "soap.wsdl")?;
    let text = |key| text_or_empty(label, fields, "soap.wsdl", key);
    let wsdl = WsdlRef {
        url: text("url")?,
        file: text("file")?,
        service: text("service")?,
        port: text("port")?,
        operation: text("operation")?,
    };
    Ok((wsdl != WsdlRef::default()).then_some(wsdl))
}

/// Limits of the GraphQL body and the `soap` section, and the kind consistency rules
/// (plan 06 §3.1). `method` is checked by [`check_method`], since sections carry none.
pub fn validate(label: &str, content: &RequestContent) -> Result<(), AppError> {
    check_body_kind(label, content.kind, &content.body)?;
    if let Body::Graphql {
        query,
        variables,
        operation_name,
    } = &content.body
    {
        check_size(label, "body.query", query)?;
        check_size(label, "body.variables", variables)?;
        check_operation_name(label, operation_name)?;
    }
    if content.kind != RequestKind::Soap && content.soap != SoapSettings::default() {
        return Err(soap_outside(label));
    }
    ws_model::validate(label, content)?;
    subscription_model::validate(label, content)?;
    validate_soap(label, &content.soap)
}

/// `model::check_method`, and POST for GraphQL in M6 (GET queries are not supported yet).
pub fn check_method(label: &str, kind: RequestKind, method: &str) -> Result<(), AppError> {
    super::model::check_method(label, method)?;
    if kind == RequestKind::Graphql && method != GRAPHQL_METHOD {
        return Err(yaml::invalid(
            label,
            "field `method` must be `POST` in a GraphQL request.",
        ));
    }
    if kind == RequestKind::Ws {
        ws_model::check_method(label, method)?;
    }
    Ok(())
}

/// The URL rule of a WebSocket request; other kinds accept any URL here.
pub fn check_url(label: &str, kind: RequestKind, url: &str) -> Result<(), AppError> {
    if kind == RequestKind::Ws {
        return ws_model::check_url(label, url);
    }
    Ok(())
}

/// The kind is fixed when the request is created (plan 06 §13 D8).
pub fn check_same_kind(
    label: &str,
    file: RequestKind,
    incoming: RequestKind,
) -> Result<(), AppError> {
    if file != incoming {
        return Err(yaml::invalid(label, "field `kind` cannot be changed."));
    }
    Ok(())
}

fn check_body_kind(label: &str, kind: RequestKind, body: &Body) -> Result<(), AppError> {
    let allowed = match kind {
        RequestKind::Graphql => matches!(body, Body::Graphql { .. }),
        RequestKind::Soap => matches!(body, Body::Xml { .. } | Body::None),
        RequestKind::Http => !matches!(body, Body::Graphql { .. }),
        RequestKind::Ws => matches!(body, Body::None),
    };
    if allowed {
        return Ok(());
    }
    let problem = match kind {
        RequestKind::Graphql => "must be `graphql` in a GraphQL request.",
        RequestKind::Soap => "must be `xml` in a SOAP request.",
        RequestKind::Http => "`graphql` needs `kind: graphql`.",
        RequestKind::Ws => "must be `none` in a WebSocket request.",
    };
    Err(invalid(label, "body.type", problem))
}

fn check_size(label: &str, field: &str, text: &str) -> Result<(), AppError> {
    if text.len() > MAX_GRAPHQL_TEXT_BYTES {
        return Err(invalid(label, field, "is larger than 1 MiB."));
    }
    Ok(())
}

fn check_operation_name(label: &str, name: &str) -> Result<(), AppError> {
    let mut chars = name.chars();
    let starts_well = chars
        .next()
        .is_none_or(|first| first == '_' || first.is_ascii_alphabetic());
    let rest_ok = chars.all(|c| c == '_' || c.is_ascii_alphanumeric());
    let short = name.chars().count() <= MAX_OPERATION_NAME_CHARS;
    if starts_well && rest_ok && short {
        return Ok(());
    }
    Err(invalid(
        label,
        "body.operation_name",
        "must be a GraphQL name of at most 256 characters.",
    ))
}

fn validate_soap(label: &str, soap: &SoapSettings) -> Result<(), AppError> {
    check_text(label, "soap.action", &soap.action, MAX_ACTION_CHARS)?;
    // The action is sent inside quotes (`SOAPAction` / the `action` parameter).
    if soap.action.contains('"') {
        return Err(invalid(label, "soap.action", "must not contain `\"`."));
    }
    let Some(wsdl) = &soap.wsdl else {
        return Ok(());
    };
    check_text(label, "soap.wsdl.url", &wsdl.url, MAX_WSDL_URL_CHARS)?;
    [
        ("soap.wsdl.file", &wsdl.file),
        ("soap.wsdl.service", &wsdl.service),
        ("soap.wsdl.port", &wsdl.port),
        ("soap.wsdl.operation", &wsdl.operation),
    ]
    .into_iter()
    .try_for_each(|(field, text)| check_text(label, field, text, MAX_WSDL_FIELD_CHARS))?;
    let is_name = !wsdl.file.contains(['/', '\\']) && wsdl.file != "." && wsdl.file != "..";
    if !is_name {
        return Err(invalid(
            label,
            "soap.wsdl.file",
            "must be a file name, not a path.",
        ));
    }
    Ok(())
}

fn soap_outside(label: &str) -> AppError {
    invalid(label, SOAP, "is only allowed in `kind: soap`.")
}

fn text(value: &str) -> Option<Value> {
    (!value.is_empty()).then(|| Value::String(value.to_string()))
}

fn fields<const N: usize>(entries: [(&str, Option<Value>); N]) -> Fields {
    entries
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key.to_string(), value)))
        .collect()
}

/// The canonical `body` of a GraphQL request; `None` when it is empty.
pub fn emit_graphql(body: &Body, raw: Option<&Value>) -> Option<Value> {
    let Body::Graphql {
        query,
        variables,
        operation_name,
    } = body
    else {
        return None;
    };
    let canonical = fields([
        ("operation_name", text(operation_name)),
        ("query", text(query)),
        ("variables", text(variables)),
    ]);
    let all = overlay(canonical, raw, &GRAPHQL_KEYS);
    if all.is_empty() {
        return None;
    }
    let typed = std::iter::once(("type".to_string(), Value::String("graphql".into())));
    Some(Value::Object(typed.chain(all).collect()))
}

/// The canonical `soap` section; `None` when all default.
pub fn emit_soap(soap: &SoapSettings, raw: Option<&Value>) -> Option<Value> {
    let version = (soap.version == SoapVersion::V12).then(|| Value::String("1.2".into()));
    let wsdl = soap.wsdl.clone().unwrap_or_default();
    let canonical_wsdl = fields([
        ("file", text(&wsdl.file)),
        ("operation", text(&wsdl.operation)),
        ("port", text(&wsdl.port)),
        ("service", text(&wsdl.service)),
        ("url", text(&wsdl.url)),
    ]);
    let all_wsdl = overlay(
        canonical_wsdl,
        raw.and_then(|raw| raw.get("wsdl")),
        &WSDL_KEYS,
    );
    let wsdl = (!all_wsdl.is_empty()).then_some(Value::Object(all_wsdl));
    let canonical = fields([
        ("action", text(&soap.action)),
        ("version", version),
        ("wsdl", wsdl),
    ]);
    let all = overlay(canonical, raw, &SOAP_KEYS);
    (!all.is_empty()).then_some(Value::Object(all))
}

#[cfg(test)]
#[path = "protocol_model_tests.rs"]
mod tests;
