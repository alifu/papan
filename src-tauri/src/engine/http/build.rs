//! `build(content, version) -> Prepared`: pure translation of a request draft into what is
//! sent (plan 03 §6). No I/O: file paths are checked against the grants by the caller.
//! M4 inserts variable resolution in front of this function.

use std::path::PathBuf;

use base64::Engine as _;
use reqwest::header::{
    HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE, SEC_WEBSOCKET_PROTOCOL, USER_AGENT,
};
use reqwest::Method;
use url::Url;

use super::digest::DigestCredentials;
use super::protocol_body;
use crate::error::AppError;
use crate::store::types::{ApiKeyPlacement, Auth, Body, KeyValue, Part, RequestContent};

const JSON_TYPE: &str = "application/json";
const XML_TYPE: &str = "application/xml";
const TEXT_TYPE: &str = "text/plain; charset=utf-8";
const FORM_TYPE: &str = "application/x-www-form-urlencoded";
const DEFAULT_SCHEME: &str = "http://";

/// A multipart part as sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartValue {
    Text(String),
    File(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedPart {
    pub name: String,
    pub value: PartValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparedBody {
    None,
    Bytes(Vec<u8>),
    Multipart(Vec<PreparedPart>),
    File(PathBuf),
}

/// Everything the client needs to send a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub method: Method,
    pub url: Url,
    /// In order, duplicates kept, defaults appended after the user's own.
    pub headers: Vec<(HeaderName, HeaderValue)>,
    pub body: PreparedBody,
    /// Headers carrying a secret beyond the well-known sensitive names (the API key
    /// header): dropped on a cross-origin redirect.
    pub secret_headers: Vec<HeaderName>,
    /// Answered by `follow` on a Digest `401`; dropped on a cross-origin redirect.
    pub digest: Option<DigestCredentials>,
}

impl Prepared {
    /// Every local file the request would upload.
    pub fn files(&self) -> Vec<PathBuf> {
        match &self.body {
            PreparedBody::File(path) => vec![path.clone()],
            PreparedBody::Multipart(parts) => parts
                .iter()
                .filter_map(|part| match &part.value {
                    PartValue::File(path) => Some(path.clone()),
                    PartValue::Text(_) => None,
                })
                .collect(),
            PreparedBody::None | PreparedBody::Bytes(_) => Vec::new(),
        }
    }
}

pub fn build(content: &RequestContent, version: &str) -> Result<Prepared, AppError> {
    let method = Method::from_bytes(content.method.as_bytes())
        .map_err(|_| AppError::invalid_input("The method is not valid."))?;
    let url = with_api_key_query(parse_url(&content.url)?, &content.auth);
    let user = user_headers(&content.headers)?;
    let (body, content_type) = body_of(&content.body)?;
    let derived = protocol_body::derived_headers(content)?;
    let content_type = content_type.filter(|_| !derived.iter().any(|(n, _)| *n == CONTENT_TYPE));
    let is_multipart = matches!(body, PreparedBody::Multipart(_));
    let api_key = api_key_header(&content.auth)?;
    let secret_headers = api_key.iter().map(|(name, _)| name.clone()).collect();
    let defaults = [
        (AUTHORIZATION, authorization(&content.auth)?),
        (CONTENT_TYPE, content_type.map(HeaderValue::from_static)),
        (USER_AGENT, Some(header_value(&format!("Papan/{version}"))?)),
    ]
    .into_iter()
    .chain(derived.into_iter().map(|(name, value)| (name, Some(value))))
    .chain(api_key)
    .filter_map(|(name, value)| value.map(|value| (name, value)));
    let headers = user
        .into_iter()
        // reqwest adds the multipart boundary header; a user Content-Type would clash.
        .filter(|(name, _)| !(is_multipart && *name == CONTENT_TYPE))
        .collect::<Vec<_>>();
    let missing: Vec<_> = defaults
        .filter(|(name, _)| !headers.iter().any(|(user_name, _)| user_name == name))
        .collect();
    Ok(Prepared {
        method,
        url,
        headers: headers.into_iter().chain(missing).collect(),
        body,
        secret_headers,
        digest: digest_credentials(&content.auth),
    })
}

/// What the WebSocket opening handshake of a `kind: ws` request carries (plan 07 §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeParts {
    /// `ws` or `wss`, with an API-key query appended.
    pub url: Url,
    /// The user's headers first, then the defaults the user did not set.
    pub headers: Vec<(HeaderName, HeaderValue)>,
    /// Headers carrying a secret beyond the well-known sensitive names.
    pub secret_headers: Vec<HeaderName>,
    /// The offered subprotocols, from the `ws` section.
    pub subprotocols: Vec<String>,
}

pub const INVALID_WS_URL_MESSAGE: &str = "Enter a valid ws:// or wss:// URL.";

fn parse_ws_url(text: &str) -> Result<Url, AppError> {
    let invalid = || AppError::invalid_input(INVALID_WS_URL_MESSAGE);
    let url =
        Url::parse(text.trim()).map_err(|source| invalid().with_detail(source.to_string()))?;
    let ok = matches!(url.scheme(), "ws" | "wss") && url.host_str().is_some_and(|h| !h.is_empty());
    if ok {
        Ok(url)
    } else {
        Err(invalid())
    }
}

/// The handshake of a resolved WebSocket request. Pure, like [`build`]: OAuth 2 has
/// already become Bearer; Digest cannot answer an upgrade and is refused.
pub fn handshake_parts(
    content: &RequestContent,
    version: &str,
) -> Result<HandshakeParts, AppError> {
    if matches!(content.auth, Auth::Digest { .. }) {
        return Err(AppError::invalid_input(
            "Digest is not supported for WebSocket.",
        ));
    }
    let url = with_api_key_query(parse_ws_url(&content.url)?, &content.auth);
    let subprotocols = content.ws.subprotocols.clone();
    let offered = (!subprotocols.is_empty())
        .then(|| header_value(&subprotocols.join(", ")))
        .transpose()?;
    let api_key = api_key_header(&content.auth)?;
    let secret_headers = api_key.iter().map(|(name, _)| name.clone()).collect();
    let defaults = [
        (AUTHORIZATION, authorization(&content.auth)?),
        (USER_AGENT, Some(header_value(&format!("Papan/{version}"))?)),
        (SEC_WEBSOCKET_PROTOCOL, offered),
    ]
    .into_iter()
    .chain(api_key)
    .filter_map(|(name, value)| value.map(|value| (name, value)));
    let user = user_headers(&content.headers)?;
    let missing: Vec<_> = defaults
        .filter(|(name, _)| !user.iter().any(|(user_name, _)| user_name == name))
        .collect();
    Ok(HandshakeParts {
        url,
        headers: user.into_iter().chain(missing).collect(),
        secret_headers,
        subprotocols,
    })
}

fn digest_credentials(auth: &Auth) -> Option<DigestCredentials> {
    match auth {
        Auth::Digest { username, password } => Some(DigestCredentials {
            username: username.clone(),
            password: password.clone(),
        }),
        _ => None,
    }
}

/// Scheme, host, port and path: never the query string, which may carry a key.
pub fn loggable_url(url: &Url) -> String {
    format!(
        "{}://{}{}{}",
        url.scheme(),
        url.host_str().unwrap_or_default(),
        url.port()
            .map(|port| format!(":{port}"))
            .unwrap_or_default(),
        url.path()
    )
}

pub const INVALID_URL_MESSAGE: &str = "Enter a valid http:// or https:// URL.";

fn invalid_url() -> AppError {
    AppError::invalid_input(INVALID_URL_MESSAGE)
}

/// The text before `:` when it looks like a URL scheme (`file`, `ftp`, …) rather than a
/// `host:port`.
fn explicit_scheme(text: &str) -> Option<&str> {
    let (scheme, rest) = text.split_once(':')?;
    let looks_like_scheme = scheme
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    let is_port = rest.chars().next().is_some_and(|c| c.is_ascii_digit());
    (looks_like_scheme && (rest.starts_with("//") || !is_port)).then_some(scheme)
}

/// The URL as sent: `http://` is assumed when no scheme is typed; only http(s).
pub fn parse_url(text: &str) -> Result<Url, AppError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(invalid_url());
    }
    let full = match explicit_scheme(trimmed) {
        Some(scheme) if ["http", "https"].contains(&scheme.to_ascii_lowercase().as_str()) => {
            trimmed.to_string()
        }
        Some(_) => return Err(invalid_url()),
        None => format!("{DEFAULT_SCHEME}{trimmed}"),
    };
    let url = Url::parse(&full).map_err(|source| invalid_url().with_detail(source.to_string()))?;
    if url.host_str().is_none_or(str::is_empty) {
        return Err(invalid_url());
    }
    Ok(url)
}

fn encode_component(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes()).collect()
}

fn with_api_key_query(url: Url, auth: &Auth) -> Url {
    let Auth::ApiKey {
        name,
        value,
        placement: ApiKeyPlacement::Query,
    } = auth
    else {
        return url;
    };
    if name.is_empty() {
        return url;
    }
    let pair = format!("{}={}", encode_component(name), encode_component(value));
    let query = match url.query() {
        Some(existing) if !existing.is_empty() => format!("{existing}&{pair}"),
        _ => pair,
    };
    let mut updated = url;
    updated.set_query(Some(&query));
    updated
}

fn header_name(name: &str) -> Result<HeaderName, AppError> {
    HeaderName::from_bytes(name.trim().as_bytes())
        .map_err(|_| AppError::invalid_input(format!("The header name `{name}` is not valid.")))
}

fn header_value(value: &str) -> Result<HeaderValue, AppError> {
    HeaderValue::from_str(value).map_err(|_| {
        AppError::invalid_input("A header value contains characters that cannot be sent.")
    })
}

fn user_headers(headers: &[KeyValue]) -> Result<Vec<(HeaderName, HeaderValue)>, AppError> {
    headers
        .iter()
        .filter(|header| header.enabled && !header.name.trim().is_empty())
        .map(|header| Ok((header_name(&header.name)?, header_value(&header.value)?)))
        .collect()
}

fn newer_papan(section: &str) -> AppError {
    AppError::invalid_input(format!(
        "This request's {section} was made by a newer Papan and cannot be sent."
    ))
}

fn authorization(auth: &Auth) -> Result<Option<HeaderValue>, AppError> {
    let text = match auth {
        Auth::Basic { username, password } => {
            let encoded =
                base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
            format!("Basic {encoded}")
        }
        Auth::Bearer { token } => format!("Bearer {token}"),
        Auth::Unsupported => return Err(newer_papan("authentication")),
        // OAuth 2 reaches `build` already turned into Bearer (plan 03b §5.3 rule 6).
        Auth::None | Auth::Oauth2 { .. } | Auth::ApiKey { .. } | Auth::Digest { .. } => {
            return Ok(None)
        }
    };
    header_value(&text).map(Some)
}

fn api_key_header(auth: &Auth) -> Result<Option<(HeaderName, Option<HeaderValue>)>, AppError> {
    match auth {
        Auth::ApiKey {
            name,
            value,
            placement: ApiKeyPlacement::Header,
        } if !name.trim().is_empty() => Ok(Some((header_name(name)?, Some(header_value(value)?)))),
        _ => Ok(None),
    }
}

fn body_of(body: &Body) -> Result<(PreparedBody, Option<&'static str>), AppError> {
    let bytes = |text: &str| PreparedBody::Bytes(text.as_bytes().to_vec());
    Ok(match body {
        Body::None => (PreparedBody::None, None),
        Body::Json { text } => (bytes(text), Some(JSON_TYPE)),
        Body::Xml { text } => (bytes(text), Some(XML_TYPE)),
        Body::Text { text } => (bytes(text), Some(TEXT_TYPE)),
        Body::Form { fields } => {
            let pairs = fields
                .iter()
                .filter(|field| field.enabled && !field.name.is_empty())
                .map(|field| (field.name.as_str(), field.value.as_str()));
            let encoded = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(pairs)
                .finish();
            (bytes(&encoded), Some(FORM_TYPE))
        }
        Body::Multipart { parts } => (PreparedBody::Multipart(multipart(parts)), None),
        Body::Binary { file: Some(path) } if !path.is_empty() => {
            (PreparedBody::File(PathBuf::from(path)), None)
        }
        Body::Binary { .. } => {
            return Err(AppError::invalid_input(
                "Choose a file for the binary body.",
            ))
        }
        Body::Unsupported => return Err(newer_papan("body")),
        Body::Graphql {
            query,
            variables,
            operation_name,
        } => {
            let json = protocol_body::graphql_json(query, variables, operation_name)?;
            (bytes(&json), Some(JSON_TYPE))
        }
    })
}

fn multipart(parts: &[Part]) -> Vec<PreparedPart> {
    parts
        .iter()
        .filter(|part| part.enabled && !part.name.is_empty())
        .map(|part| PreparedPart {
            name: part.name.clone(),
            value: match part.file.as_deref().filter(|file| !file.is_empty()) {
                Some(file) => PartValue::File(PathBuf::from(file)),
                None => PartValue::Text(part.value.clone()),
            },
        })
        .collect()
}

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "build_ws_tests.rs"]
mod ws_tests;
