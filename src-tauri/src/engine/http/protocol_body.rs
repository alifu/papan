//! Bodies and derived headers of GraphQL and SOAP requests (plan 06 §5). Pure: `build`
//! calls in here, and the usual rule applies that a user header of the same name wins.

use reqwest::header::{HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE};

use crate::error::AppError;
use crate::store::types::{Body, RequestContent, RequestKind, SoapSettings, SoapVersion};

const GRAPHQL_ACCEPT: &str = "application/graphql-response+json, application/json";
const SOAP_11_TYPE: &str = "text/xml; charset=utf-8";
const SOAP_12_TYPE: &str = "application/soap+xml; charset=utf-8";
const SOAP_ACTION: &str = "soapaction";
const VARIABLES_MESSAGE: &str = "GraphQL variables must be a JSON object.";

/// `{"query", "operationName"?, "variables"?}`. The variables text is embedded as typed
/// (after checking that it is a JSON object), so key order and big numbers survive.
pub fn graphql_json(
    query: &str,
    variables: &str,
    operation_name: &str,
) -> Result<String, AppError> {
    let quoted = |text: &str| serde_json::Value::String(text.to_string()).to_string();
    let operation = if operation_name.is_empty() {
        String::new()
    } else {
        format!(",\"operationName\":{}", quoted(operation_name))
    };
    let variables = variables.trim();
    let variables = if variables.is_empty() {
        String::new()
    } else {
        check_variables(variables)?;
        format!(",\"variables\":{variables}")
    };
    Ok(format!(
        "{{\"query\":{}{operation}{variables}}}",
        quoted(query)
    ))
}

fn check_variables(text: &str) -> Result<(), AppError> {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(serde_json::Value::Object(_)) => Ok(()),
        Ok(_) => Err(AppError::invalid_input(VARIABLES_MESSAGE)),
        Err(source) => {
            Err(AppError::invalid_input(VARIABLES_MESSAGE).with_detail(source.to_string()))
        }
    }
}

/// `pm.request.body` of a GraphQL request: the JSON text as sent (`None` while the
/// variables are not a JSON object; the send then reports that).
pub fn graphql_script_text(query: &str, variables: &str, operation_name: &str) -> Option<String> {
    graphql_json(query, variables, operation_name).ok()
}

/// A GraphQL body from the text a script set: a JSON object with a string `query` maps back
/// (variables pretty-printed), anything else becomes a text body.
pub fn graphql_from_script(text: String) -> Body {
    let parsed = serde_json::from_str::<serde_json::Value>(&text).ok();
    let Some(serde_json::Value::Object(object)) = parsed else {
        return Body::Text { text };
    };
    let Some(query) = object.get("query").and_then(serde_json::Value::as_str) else {
        return Body::Text { text };
    };
    let variables = match object.get("variables") {
        None | Some(serde_json::Value::Null) => String::new(),
        Some(value) => serde_json::to_string_pretty(value).unwrap_or_default(),
    };
    let operation_name = object
        .get("operationName")
        .and_then(serde_json::Value::as_str);
    Body::Graphql {
        query: query.to_string(),
        variables,
        operation_name: operation_name.unwrap_or_default().to_string(),
    }
}

/// Headers the request's kind adds: `Accept` for GraphQL; `Content-Type` and, for 1.1,
/// `SOAPAction` for SOAP. A derived `Content-Type` replaces the body's own.
pub fn derived_headers(
    content: &RequestContent,
) -> Result<Vec<(HeaderName, HeaderValue)>, AppError> {
    match content.kind {
        RequestKind::Http => Ok(Vec::new()),
        RequestKind::Graphql => Ok(vec![(ACCEPT, HeaderValue::from_static(GRAPHQL_ACCEPT))]),
        RequestKind::Soap => soap_headers(&content.soap),
        RequestKind::Ws | RequestKind::Grpc => Ok(Vec::new()),
    }
}

fn soap_headers(soap: &SoapSettings) -> Result<Vec<(HeaderName, HeaderValue)>, AppError> {
    let value = |text: String| {
        HeaderValue::from_str(&text).map_err(|_| {
            AppError::invalid_input("The SOAP action contains characters that cannot be sent.")
        })
    };
    let action = &soap.action;
    // Sent inside quotes: a quote (or a control character) would break the header.
    if action.contains(|c: char| c == '"' || c.is_control()) {
        return Err(AppError::invalid_input(
            "The SOAP action contains characters that cannot be sent.",
        ));
    }
    match soap.version {
        SoapVersion::V11 => Ok(vec![
            (CONTENT_TYPE, HeaderValue::from_static(SOAP_11_TYPE)),
            (
                HeaderName::from_static(SOAP_ACTION),
                value(format!("\"{action}\""))?,
            ),
        ]),
        SoapVersion::V12 if action.is_empty() => {
            Ok(vec![(CONTENT_TYPE, HeaderValue::from_static(SOAP_12_TYPE))])
        }
        SoapVersion::V12 => Ok(vec![(
            CONTENT_TYPE,
            value(format!("{SOAP_12_TYPE}; action=\"{action}\""))?,
        )]),
    }
}

#[cfg(test)]
#[path = "protocol_body_tests.rs"]
mod tests;
