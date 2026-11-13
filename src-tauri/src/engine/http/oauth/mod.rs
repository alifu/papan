//! OAuth 2 (plan 03b §5, phase B): client credentials and authorization code with PKCE.
//!
//! [`OAuthConfig`] is the resolved configuration of one request; its [`config_key`]
//! names the stored token, so a changed URL, client or scope never reuses a token.

mod authorize;
pub mod commands;
#[cfg(test)]
pub(crate) mod fixtures;
pub mod flow;
pub mod listener;
mod mask_set;
pub mod pkce;
pub mod token;
pub mod tokens;

use std::fmt;

use url::Url;

use super::resolve::Resolved;
use crate::error::AppError;
use crate::store::types::{Auth, ClientAuth, OAuthGrant};
use crate::vars::template;

pub use flow::OAuth;

/// Opens the authorize URL in the user's browser (production: the opener plugin).
pub trait BrowserOpener: Send + Sync {
    fn open(&self, url: &Url) -> Result<(), AppError>;
}

/// The plan names this error `conflict`; `ErrorCode` has no such code yet, so it is
/// reported as `invalid_input` from this one place.
pub(crate) fn conflict(message: impl Into<String>) -> AppError {
    AppError::invalid_input(message)
}

/// A fixed redirect port below this is refused (privileged ports).
const MIN_REDIRECT_PORT: u16 = 1024;

/// The resolved OAuth 2 fields of a request.
#[derive(Clone, PartialEq, Eq)]
pub struct OAuthConfig {
    pub grant: OAuthGrant,
    /// Parsed for `AuthorizationCode` only.
    pub auth_url: Option<Url>,
    pub token_url: Url,
    pub client_id: String,
    pub client_secret: String,
    pub scope: String,
    pub client_auth: ClientAuth,
    pub redirect_port: u16,
    /// The URLs as resolved (trimmed), which is what the key is derived from.
    auth_text: String,
    token_text: String,
}

impl fmt::Debug for OAuthConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthConfig")
            .field("grant", &self.grant)
            .field("token_host", &host_of(&self.token_url))
            .finish_non_exhaustive()
    }
}

/// Provider text as printable characters. It is not cut here: the flow masks it first
/// and bounds the length afterwards, so a secret is never left half-masked.
pub(crate) fn plain(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

/// `host` or `host:port` of `url`, for messages and logs (never the path or query).
pub fn host_of(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    }
}

/// `http` or `https` with a host; anything else never reaches the opener or the network.
fn endpoint(text: &str, label: &str) -> Result<Url, AppError> {
    Url::parse(text)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https") && url.has_host())
        .filter(|url| url.username().is_empty() && url.password().is_none())
        .ok_or_else(|| {
            AppError::invalid_input(format!(
                "The {label} must be an http:// or https:// address."
            ))
        })
}

fn undefined(resolved: &Resolved, fields: &[&str]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for name in fields.iter().flat_map(|field| template::names(field)) {
        let missing = resolved
            .unresolved
            .iter()
            .any(|unresolved| unresolved == name);
        if missing && !names.iter().any(|known| known == name) {
            names.push(name.to_string());
        }
    }
    names
}

impl OAuthConfig {
    /// The configuration of a resolved draft; `None` when its auth is not OAuth 2.
    /// An unresolved variable or a URL that is not `http(s)` is refused here, before
    /// anything is opened or sent.
    pub fn from_resolved(resolved: &Resolved) -> Result<Option<Self>, AppError> {
        let Auth::Oauth2 {
            grant,
            auth_url,
            token_url,
            client_id,
            client_secret,
            scope,
            client_auth,
            redirect_port,
        } = &resolved.content.auth
        else {
            return Ok(None);
        };
        let with_browser = *grant == OAuthGrant::AuthorizationCode;
        let auth_text = if with_browser { auth_url.trim() } else { "" };
        let token_text = token_url.trim();
        let missing = undefined(
            resolved,
            &[auth_text, token_text, client_id, client_secret, scope],
        );
        if !missing.is_empty() {
            let names: Vec<String> = missing.iter().map(|name| format!("`{name}`")).collect();
            return Err(AppError::invalid_input(format!(
                "Not defined: {}. Select an environment or define it.",
                names.join(", ")
            )));
        }
        let auth_url = with_browser
            .then(|| endpoint(auth_text, "authorization URL"))
            .transpose()?;
        if with_browser && !(*redirect_port == 0 || *redirect_port >= MIN_REDIRECT_PORT) {
            return Err(AppError::invalid_input(
                "The redirect port must be 0 or between 1024 and 65535.",
            ));
        }
        Ok(Some(Self {
            grant: *grant,
            auth_url,
            token_url: endpoint(token_text, "token URL")?,
            client_id: client_id.clone(),
            client_secret: client_secret.clone(),
            scope: scope.trim().to_string(),
            client_auth: *client_auth,
            redirect_port: *redirect_port,
            auth_text: auth_text.to_string(),
            token_text: token_text.to_string(),
        }))
    }

    /// Whether either endpoint is plain `http`.
    pub fn plaintext(&self) -> bool {
        std::iter::once(&self.token_url)
            .chain(&self.auth_url)
            .any(|url| url.scheme() == "http")
    }
}

/// SHA-256 hex over a length-prefixed encoding of grant, `auth_url`, `token_url`,
/// `client_id`, `scope` and `client_auth`. Never the client secret: the key is shown
/// to the webview.
pub fn config_key(config: &OAuthConfig) -> String {
    let grant = match config.grant {
        OAuthGrant::ClientCredentials => "client_credentials",
        OAuthGrant::AuthorizationCode => "authorization_code",
    };
    let client_auth = match config.client_auth {
        ClientAuth::Basic => "basic",
        ClientAuth::Body => "body",
    };
    let fields = [
        grant,
        &config.auth_text,
        &config.token_text,
        &config.client_id,
        &config.scope,
        client_auth,
    ];
    let mut context = aws_lc_rs::digest::Context::new(&aws_lc_rs::digest::SHA256);
    for field in fields {
        context.update(&(field.len() as u64).to_be_bytes());
        context.update(field.as_bytes());
    }
    context
        .finish()
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
