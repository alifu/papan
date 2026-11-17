//! Token endpoint calls (code exchange, client credentials, refresh) and response
//! parsing (plan 03b §5.3.5). `POST` form, `Accept: application/json`, never a cookie
//! and never a followed redirect (the client is built without a jar and with no
//! redirect policy), 30 s, at most 1 MiB. Error texts here may quote the provider at
//! any length: the flow masks them with its own secret set, then bounds them.

use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Client;
use serde_json::Value;
use url::Url;
use zeroize::Zeroizing;

use super::super::send::{map_error, timed_out};
use super::{host_of, plain, OAuthConfig};
use crate::error::AppError;
use crate::store::types::ClientAuth;

pub const TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// What is asked of the token endpoint.
#[derive(Clone, Copy)]
pub enum Grant<'a> {
    ClientCredentials,
    Code {
        code: &'a str,
        verifier: &'a str,
        redirect_uri: &'a str,
    },
    Refresh {
        refresh_token: &'a str,
    },
}

/// A successful token response.
#[derive(Clone, PartialEq, Eq)]
pub struct Issued {
    pub access_token: Zeroizing<String>,
    pub refresh_token: Option<Zeroizing<String>>,
    pub expires_in_s: Option<u64>,
    pub scope: Option<String>,
}

impl std::fmt::Debug for Issued {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Issued")
            .field("expires_in_s", &self.expires_in_s)
            .field("has_refresh_token", &self.refresh_token.is_some())
            .finish_non_exhaustive()
    }
}

fn encoded(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes()).collect()
}

/// The `Authorization` value of RFC 6749 §2.3.1: both parts form-encoded first.
fn basic(client_id: &str, client_secret: &str) -> Zeroizing<String> {
    let pair = Zeroizing::new(format!("{}:{}", encoded(client_id), encoded(client_secret)));
    Zeroizing::new(format!("Basic {}", STANDARD.encode(pair.as_bytes())))
}

/// A public client (no secret) is always named in the body.
fn in_header(config: &OAuthConfig) -> bool {
    config.client_auth == ClientAuth::Basic && !config.client_secret.is_empty()
}

/// The form fields of one request.
fn form(config: &OAuthConfig, grant: Grant<'_>) -> Vec<(&'static str, String)> {
    let mut fields: Vec<(&'static str, String)> = match grant {
        Grant::ClientCredentials => vec![("grant_type", "client_credentials".to_string())],
        Grant::Code {
            code,
            verifier,
            redirect_uri,
        } => vec![
            ("grant_type", "authorization_code".to_string()),
            ("code", code.to_string()),
            ("redirect_uri", redirect_uri.to_string()),
            ("code_verifier", verifier.to_string()),
        ],
        Grant::Refresh { refresh_token } => vec![
            ("grant_type", "refresh_token".to_string()),
            ("refresh_token", refresh_token.to_string()),
        ],
    };
    if matches!(grant, Grant::ClientCredentials) && !config.scope.is_empty() {
        fields.push(("scope", config.scope.clone()));
    }
    if !in_header(config) {
        fields.push(("client_id", config.client_id.clone()));
        if !config.client_secret.is_empty() {
            fields.push(("client_secret", config.client_secret.clone()));
        }
    }
    fields
}

fn refused(status: u16, body: &[u8]) -> AppError {
    if (300..400).contains(&status) {
        return AppError::invalid_input(
            "The token endpoint answered with a redirect, which is not followed. \
             Check the token URL.",
        );
    }
    let json: Option<Value> = serde_json::from_slice(body).ok();
    let field = |name: &str| {
        json.as_ref()
            .and_then(|json| json.get(name))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    match field("error") {
        Some(error) => {
            let description = field("error_description").unwrap_or_default();
            let text = plain(&format!("{error} {description}"));
            AppError::invalid_input(format!("The token endpoint refused the request: {text}"))
        }
        None => AppError::invalid_input(format!("The token endpoint answered {status}.")),
    }
}

fn unusable(reason: &str) -> AppError {
    AppError::invalid_input(format!(
        "The token endpoint's answer cannot be used: {reason}"
    ))
}

/// Seconds as a JSON number or, as some providers send it, a numeric string.
fn seconds(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .or_else(|| number.as_f64().filter(|s| *s >= 0.0).map(|s| s as u64)),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

/// Reads a token endpoint response. Only `token_type: bearer` is accepted.
pub fn parse(status: u16, body: &[u8]) -> Result<Issued, AppError> {
    if !(200..300).contains(&status) {
        return Err(refused(status, body));
    }
    let json: Value = serde_json::from_slice(body).map_err(|_| unusable("it is not JSON."))?;
    let text = |name: &str| json.get(name).and_then(Value::as_str);
    let token_type = text("token_type").unwrap_or_default();
    if !token_type.eq_ignore_ascii_case("bearer") {
        return Err(unusable("only bearer tokens are supported."));
    }
    let access_token = text("access_token")
        .filter(|token| !token.is_empty())
        .ok_or_else(|| unusable("it has no access token."))?;
    Ok(Issued {
        access_token: Zeroizing::new(access_token.to_string()),
        refresh_token: text("refresh_token")
            .filter(|token| !token.is_empty())
            .map(|token| Zeroizing::new(token.to_string())),
        expires_in_s: json.get("expires_in").and_then(seconds),
        scope: text("scope").map(str::to_string),
    })
}

/// The endpoint's raw answer.
pub struct Reply {
    pub status: u16,
    body: Zeroizing<Vec<u8>>,
}

impl Reply {
    /// The token in the answer, or why it cannot be used.
    pub fn issued(&self) -> Result<Issued, AppError> {
        parse(self.status, &self.body)
    }

    /// Whether the provider turned the grant or the client down for good (a `4xx`
    /// other than "try again later"), as opposed to failing itself.
    pub fn rejected(&self) -> bool {
        (400..500).contains(&self.status) && !matches!(self.status, 408 | 429)
    }
}

async fn exchange(
    client: &Client,
    token_url: &Url,
    config: &OAuthConfig,
    grant: Grant<'_>,
) -> Result<Reply, AppError> {
    let body = Zeroizing::new(
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form(config, grant))
            .finish(),
    );
    let request = client
        .post(token_url.clone())
        .header(ACCEPT, "application/json")
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(body.as_bytes().to_vec());
    let request = if in_header(config) {
        let value = basic(&config.client_id, &config.client_secret);
        let mut header = reqwest::header::HeaderValue::from_str(&value)
            .map_err(|_| AppError::invalid_input("The client id or secret cannot be sent."))?;
        header.set_sensitive(true);
        request.header(AUTHORIZATION, header)
    } else {
        request
    };
    let mut response = request
        .send()
        .await
        .map_err(|error| map_error(error, token_url))?;
    let status = response.status().as_u16();
    let mut body = Zeroizing::new(Vec::new());
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| map_error(error, token_url))?
    {
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(unusable("it is larger than 1 MiB."));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Reply { status, body })
}

/// Sends `grant` to `token_url` with the client authentication of `config`.
pub async fn send(
    client: &Client,
    token_url: &Url,
    config: &OAuthConfig,
    grant: Grant<'_>,
) -> Result<Reply, AppError> {
    tracing::info!(host = %host_of(token_url), "requesting an OAuth token");
    tokio::time::timeout(TIMEOUT, exchange(client, token_url, config, grant))
        .await
        .unwrap_or_else(|_| Err(timed_out()))
}

/// [`send`], then the token in the answer.
pub async fn request(
    client: &Client,
    token_url: &Url,
    config: &OAuthConfig,
    grant: Grant<'_>,
) -> Result<Issued, AppError> {
    send(client, token_url, config, grant).await?.issued()
}

#[cfg(test)]
#[path = "token_tests.rs"]
mod tests;
