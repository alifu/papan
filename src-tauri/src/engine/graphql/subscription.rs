//! Turning a resolved GraphQL request into a subscription socket (plan 06b §5): the
//! `ws(s)` URL, the connect spec, and the two frames the driver sends.

use url::Url;

use super::gql_ws::{check_init_payload, SUBPROTOCOL};
use crate::engine::http::protocol_body::graphql_json;
use crate::engine::http::resolve::Resolved;
use crate::engine::http::HttpEngine;
use crate::engine::ws::handshake::{connect_spec, WsConnectSpec};
use crate::error::AppError;
use crate::store::types::{Body, RequestContent, RequestLocation};
use crate::vars::template;

const SCHEME_MESSAGE: &str = "A subscription URL must start with http, https, ws or wss.";

/// What the driver needs besides the socket: both texts may hold secrets.
pub struct SubscriptionFrames {
    pub init_payload: Option<String>,
    pub body_json: String,
}

/// `subscription.url` if set, else `url` with `http` -> `ws` and `https` -> `wss`.
pub fn ws_url(content: &RequestContent) -> Result<Url, AppError> {
    let typed = content.subscription.url.trim();
    let text = if typed.is_empty() {
        content.url.trim()
    } else {
        typed
    };
    let mut url = Url::parse(text).map_err(|_| AppError::invalid_input(SCHEME_MESSAGE))?;
    let scheme = match url.scheme() {
        "http" | "ws" => "ws",
        "https" | "wss" => "wss",
        _ => return Err(AppError::invalid_input(SCHEME_MESSAGE)),
    };
    url.set_scheme(scheme)
        .map_err(|()| AppError::invalid_input(SCHEME_MESSAGE))?;
    Ok(url)
}

fn not_defined(names: &[&str]) -> AppError {
    let names: Vec<String> = names.iter().map(|n| format!("`{n}`")).collect();
    AppError::invalid_input(format!(
        "Not defined: {}. Select an environment or define it.",
        names.join(", ")
    ))
}

/// A variable used by the subscription that did not resolve is an error, as in a send.
fn check_defined(typed: &RequestContent, resolved: &Resolved) -> Result<(), AppError> {
    let variables = match &typed.body {
        Body::Graphql { variables, .. } => variables.as_str(),
        _ => "",
    };
    let missing: Vec<&str> = [&typed.subscription.url, &typed.url]
        .into_iter()
        .map(String::as_str)
        .chain([typed.subscription.init_payload.as_str(), variables])
        .flat_map(template::names)
        .filter(|name| resolved.unresolved.iter().any(|m| m == name))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(not_defined(&missing))
    }
}

/// The frames of a resolved request: the init payload (checked JSON object) and the
/// `subscribe` payload.
pub fn frames(resolved: &Resolved) -> Result<SubscriptionFrames, AppError> {
    let content = &resolved.content;
    let Body::Graphql {
        query,
        variables,
        operation_name,
    } = &content.body
    else {
        return Err(AppError::invalid_input("This is not a GraphQL request."));
    };
    let init_payload = check_init_payload(&content.subscription.init_payload)?.map(str::to_string);
    Ok(SubscriptionFrames {
        init_payload,
        body_json: graphql_json(query, variables, operation_name)?,
    })
}

/// The spec for `resolved` (OAuth 2, cookies, proxy and client certificate as in M7).
/// `typed` is the draft before resolution. Errors are masked by `connect_spec`.
pub async fn subscription_spec(
    http: &HttpEngine,
    location: &RequestLocation,
    typed: &RequestContent,
    resolved: &Resolved,
    version: &str,
) -> Result<WsConnectSpec, AppError> {
    check_defined(typed, resolved)?;
    let mut content = resolved.content.clone();
    content.url = ws_url(&resolved.content)?.to_string();
    content.ws.subprotocols = vec![SUBPROTOCOL.to_string()];
    let typed_url = match typed.subscription.url.trim() {
        "" => typed.url.clone(),
        own => own.to_string(),
    };
    let for_ws = Resolved {
        content,
        template_url: typed_url,
        ..resolved.clone()
    };
    connect_spec(http, location, &for_ws, version).await
}

#[cfg(test)]
#[path = "subscription_tests.rs"]
mod tests;
