//! The `auth` section of a request file (plan 03 §3, 03b §3): parsing from the raw YAML
//! value and validation of the typed value, including the Digest and OAuth 2 limits.

use serde_json::Value;

use super::request_model::{
    as_map, check_text, invalid, opt_text, present, req_text, text_or_empty, Fields, AUTH,
    MAX_ENTRY_VALUE_CHARS,
};
use super::types::{ApiKeyPlacement, Auth, ClientAuth, OAuthGrant};
use crate::error::AppError;

pub const MAX_OAUTH_URL_CHARS: usize = 2048;
pub const MAX_CLIENT_ID_CHARS: usize = 512;
pub const MAX_SCOPE_CHARS: usize = 1024;
/// A fixed redirect port must not be a privileged one; `0` means any free port.
pub const MIN_FIXED_REDIRECT_PORT: u16 = 1024;

const REDIRECT_PORT: &str = "auth.redirect_port";
const REDIRECT_PORT_RULE: &str = "must be 0 or a whole number from 1024 to 65535.";

pub fn parse_auth(label: &str, value: Option<&Value>) -> Result<Auth, AppError> {
    let Some(value) = present(value) else {
        return Ok(Auth::None);
    };
    let fields = as_map(label, value, AUTH)?;
    let text = |key| text_or_empty(label, fields, AUTH, key);
    Ok(match req_text(label, fields, AUTH, "type")?.as_str() {
        "none" => Auth::None,
        "basic" => Auth::Basic {
            username: text("username")?,
            password: text("password")?,
        },
        "bearer" => Auth::Bearer {
            token: text("token")?,
        },
        "api_key" => Auth::ApiKey {
            name: text("name")?,
            value: text("value")?,
            placement: parse_placement(label, fields)?,
        },
        "digest" => Auth::Digest {
            username: text("username")?,
            password: text("password")?,
        },
        "oauth2" => parse_oauth2(label, fields)?,
        _ => Auth::Unsupported,
    })
}

fn parse_placement(label: &str, fields: &Fields) -> Result<ApiKeyPlacement, AppError> {
    match opt_text(label, fields, AUTH, "placement")?.as_deref() {
        None | Some("header") => Ok(ApiKeyPlacement::Header),
        Some("query") => Ok(ApiKeyPlacement::Query),
        Some(_) => Err(invalid(
            label,
            "auth.placement",
            "must be `header` or `query`.",
        )),
    }
}

/// `auth_url` is read for `authorization_code` only (and required there), as it is written.
fn parse_oauth2(label: &str, fields: &Fields) -> Result<Auth, AppError> {
    let text = |key| text_or_empty(label, fields, AUTH, key);
    let grant = parse_grant(label, fields)?;
    let auth_url = match grant {
        OAuthGrant::AuthorizationCode => req_text(label, fields, AUTH, "auth_url")?,
        OAuthGrant::ClientCredentials => String::new(),
    };
    Ok(Auth::Oauth2 {
        grant,
        auth_url,
        token_url: req_text(label, fields, AUTH, "token_url")?,
        client_id: text("client_id")?,
        client_secret: text("client_secret")?,
        scope: text("scope")?,
        client_auth: parse_client_auth(label, fields)?,
        redirect_port: parse_redirect_port(label, fields)?,
    })
}

fn parse_grant(label: &str, fields: &Fields) -> Result<OAuthGrant, AppError> {
    match req_text(label, fields, AUTH, "grant")?.as_str() {
        "client_credentials" => Ok(OAuthGrant::ClientCredentials),
        "authorization_code" => Ok(OAuthGrant::AuthorizationCode),
        _ => Err(invalid(
            label,
            "auth.grant",
            "must be `client_credentials` or `authorization_code`.",
        )),
    }
}

fn parse_client_auth(label: &str, fields: &Fields) -> Result<ClientAuth, AppError> {
    match opt_text(label, fields, AUTH, "client_auth")?.as_deref() {
        None | Some("basic") => Ok(ClientAuth::Basic),
        Some("body") => Ok(ClientAuth::Body),
        Some(_) => Err(invalid(
            label,
            "auth.client_auth",
            "must be `basic` or `body`.",
        )),
    }
}

fn parse_redirect_port(label: &str, fields: &Fields) -> Result<u16, AppError> {
    let Some(value) = present(fields.get("redirect_port")) else {
        return Ok(0);
    };
    let port = value
        .as_u64()
        .and_then(|port| u16::try_from(port).ok())
        .ok_or_else(|| invalid(label, REDIRECT_PORT, REDIRECT_PORT_RULE))?;
    check_redirect_port(label, port)?;
    Ok(port)
}

fn check_redirect_port(label: &str, port: u16) -> Result<(), AppError> {
    if port != 0 && port < MIN_FIXED_REDIRECT_PORT {
        return Err(invalid(label, REDIRECT_PORT, REDIRECT_PORT_RULE));
    }
    Ok(())
}

pub fn validate_auth(label: &str, auth: &Auth) -> Result<(), AppError> {
    let value = MAX_ENTRY_VALUE_CHARS;
    let fields: Vec<(&str, &str, usize)> = match auth {
        Auth::Basic { username, password } | Auth::Digest { username, password } => vec![
            ("auth.username", username, value),
            ("auth.password", password, value),
        ],
        Auth::Bearer { token } => vec![("auth.token", token, value)],
        Auth::ApiKey {
            name, value: key, ..
        } => {
            vec![("auth.name", name, value), ("auth.value", key, value)]
        }
        Auth::Oauth2 {
            auth_url,
            token_url,
            client_id,
            client_secret,
            scope,
            redirect_port,
            ..
        } => {
            check_redirect_port(label, *redirect_port)?;
            vec![
                ("auth.auth_url", auth_url, MAX_OAUTH_URL_CHARS),
                ("auth.token_url", token_url, MAX_OAUTH_URL_CHARS),
                ("auth.client_id", client_id, MAX_CLIENT_ID_CHARS),
                ("auth.client_secret", client_secret, value),
                ("auth.scope", scope, MAX_SCOPE_CHARS),
            ]
        }
        Auth::None | Auth::Unsupported => Vec::new(),
    };
    fields
        .into_iter()
        .try_for_each(|(field, text, max)| check_text(label, field, text, max))
}

/// `auth` as it reads back after a write: a client-credentials grant has no `auth_url`.
pub fn normalise_auth(auth: &Auth) -> Auth {
    match auth {
        Auth::Oauth2 {
            grant: grant @ OAuthGrant::ClientCredentials,
            token_url,
            client_id,
            client_secret,
            scope,
            client_auth,
            redirect_port,
            ..
        } => Auth::Oauth2 {
            grant: *grant,
            auth_url: String::new(),
            token_url: token_url.clone(),
            client_id: client_id.clone(),
            client_secret: client_secret.clone(),
            scope: scope.clone(),
            client_auth: *client_auth,
            redirect_port: *redirect_port,
        },
        other => other.clone(),
    }
}

#[cfg(test)]
#[path = "auth_model_tests.rs"]
mod tests;
