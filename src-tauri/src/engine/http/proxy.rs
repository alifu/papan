//! The app proxy (plan 03b §4.1): validation of what Network settings writes, and how a
//! client is built with it. The password is only ever given to `Proxy::basic_auth`,
//! never put into the proxy URL that is stored or logged.

use hyper_util::client::proxy::matcher::Matcher;
use reqwest::{ClientBuilder, NoProxy, Proxy};
use serde::{Deserialize, Serialize};
use url::Url;

use super::types::{ProxyInput, ProxyMode};
use crate::error::AppError;

pub const MAX_BYPASS: usize = 100;
const MAX_PATTERN_CHARS: usize = 253;
const MAX_USERNAME_CHARS: usize = 256;
const MAX_URL_CHARS: usize = 2048;

const BAD_URL: &str = "Enter the proxy as http://host:port or https://host:port, \
    without user name, path or query.";
const NO_URL: &str = "Enter the proxy URL.";
const BAD_USERNAME: &str = "The proxy user name is too long or contains control characters.";
const TOO_MANY_BYPASS: &str = "At most 100 bypass patterns are allowed.";
const BAD_BYPASS: &str = "A bypass pattern may not contain spaces or commas.";

/// The proxy as kept in `network.json`; the password is in the vault.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredProxy {
    #[serde(default)]
    pub mode: ProxyMode,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub bypass: Vec<String>,
}

/// `url` as a proxy: `http` or `https`, a host, no userinfo, path, query or fragment.
fn parse_url(url: &str) -> Result<Url, AppError> {
    let parsed = Url::parse(url).map_err(|_| AppError::invalid_input(BAD_URL))?;
    let bare = matches!(parsed.scheme(), "http" | "https")
        && parsed.host_str().is_some_and(|host| !host.is_empty())
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && matches!(parsed.path(), "" | "/")
        && parsed.query().is_none()
        && parsed.fragment().is_none();
    if bare {
        Ok(parsed)
    } else {
        Err(AppError::invalid_input(BAD_URL))
    }
}

fn valid_pattern(pattern: &str) -> bool {
    pattern.chars().count() <= MAX_PATTERN_CHARS
        && !pattern.chars().any(|c| c == ',' || c.is_whitespace())
}

/// The input as it will be stored. The URL is required for `custom` only, but checked
/// whenever it is given.
pub fn validate(input: &ProxyInput) -> Result<StoredProxy, AppError> {
    let url = input.url.trim();
    if url.chars().count() > MAX_URL_CHARS {
        return Err(AppError::invalid_input(BAD_URL));
    }
    if !url.is_empty() {
        parse_url(url)?;
    } else if input.mode == ProxyMode::Custom {
        return Err(AppError::invalid_input(NO_URL));
    }
    let username = input.username.trim();
    if username.chars().count() > MAX_USERNAME_CHARS || username.chars().any(char::is_control) {
        return Err(AppError::invalid_input(BAD_USERNAME));
    }
    let bypass: Vec<String> = input
        .bypass
        .iter()
        .map(|pattern| pattern.trim())
        .filter(|pattern| !pattern.is_empty())
        .map(str::to_string)
        .collect();
    if bypass.len() > MAX_BYPASS {
        return Err(AppError::invalid_input(TOO_MANY_BYPASS));
    }
    if !bypass.iter().all(|pattern| valid_pattern(pattern)) {
        return Err(AppError::invalid_input(BAD_BYPASS));
    }
    Ok(StoredProxy {
        mode: input.mode,
        url: url.to_string(),
        username: username.to_string(),
        bypass,
    })
}

/// A stored proxy read back from disk is used only if it would still be accepted.
pub fn is_valid(stored: &StoredProxy) -> bool {
    let input = ProxyInput {
        mode: stored.mode,
        url: stored.url.clone(),
        username: stored.username.clone(),
        password: crate::vars::types::SecretEdit::Keep,
        bypass: stored.bypass.clone(),
    };
    validate(&input).is_ok_and(|checked| checked == *stored)
}

/// The proxy host, for logs (never the URL, which could one day carry more).
pub fn host_of(stored: &StoredProxy) -> String {
    parse_url(&stored.url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_default()
}

/// `host:port` of the proxy URL, which the stored password is bound to; empty without one.
pub fn endpoint_of(stored: &StoredProxy) -> String {
    parse_url(&stored.url)
        .ok()
        .and_then(|url| {
            Some(format!(
                "{}:{}",
                url.host_str()?,
                url.port_or_known_default()?
            ))
        })
        .unwrap_or_default()
}

/// The bypass list as `NO_PROXY` text: a domain matches its subdomains; `*.x` is `.x`.
fn no_proxy_list(bypass: &[String]) -> String {
    let list: Vec<&str> = bypass
        .iter()
        .map(|pattern| pattern.strip_prefix("*.").unwrap_or(pattern))
        .collect();
    list.join(",")
}

fn no_proxy(bypass: &[String]) -> Option<NoProxy> {
    if bypass.is_empty() {
        return None;
    }
    NoProxy::from_string(&no_proxy_list(bypass))
}

/// Whether a request to `url` goes through an `https://` proxy, decided by the matcher
/// reqwest itself uses (`system` reads the environment and the OS settings).
pub fn is_https_for(stored: &StoredProxy, url: &Url) -> bool {
    let matcher = match stored.mode {
        ProxyMode::None => return false,
        ProxyMode::System => Matcher::from_system(),
        ProxyMode::Custom => Matcher::builder()
            .all(stored.url.clone())
            .no(no_proxy_list(&stored.bypass))
            .build(),
    };
    let Ok(destination) = url.as_str().parse::<http::Uri>() else {
        return false;
    };
    matcher
        .intercept(&destination)
        .is_some_and(|proxy| proxy.uri().scheme_str() == Some("https"))
}

/// `builder` with the proxy mode applied. `system` keeps reqwest's own behaviour.
pub fn apply(
    builder: ClientBuilder,
    stored: &StoredProxy,
    password: Option<&str>,
) -> Result<ClientBuilder, AppError> {
    match stored.mode {
        ProxyMode::System => Ok(builder),
        ProxyMode::None => Ok(builder.no_proxy()),
        ProxyMode::Custom => {
            let url = parse_url(&stored.url)?;
            let proxy = Proxy::all(url.as_str())
                .map_err(|_| AppError::invalid_input(BAD_URL))?
                .no_proxy(no_proxy(&stored.bypass));
            let proxy = match (stored.username.as_str(), password) {
                ("", None) => proxy,
                (username, password) => proxy.basic_auth(username, password.unwrap_or_default()),
            };
            Ok(builder.proxy(proxy))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vars::types::SecretEdit;

    fn input(mode: ProxyMode, url: &str, bypass: &[&str]) -> ProxyInput {
        ProxyInput {
            mode,
            url: url.to_string(),
            username: " alifu ".to_string(),
            password: SecretEdit::Keep,
            bypass: bypass.iter().map(|p| p.to_string()).collect(),
        }
    }

    #[test]
    fn a_custom_proxy_url_is_host_and_port_only() {
        for bad in [
            "ftp://proxy:21",
            "http://user:pw@proxy:3128",
            "http://proxy:3128/path",
            "http://proxy:3128/?q=1",
            "http://proxy:3128/#f",
            "proxy:3128",
        ] {
            let result = validate(&input(ProxyMode::Custom, bad, &[]));

            assert!(result.is_err(), "{bad}");
        }
        let ok = validate(&input(ProxyMode::Custom, " http://proxy.corp:3128 ", &[])).unwrap();
        assert_eq!(
            (ok.url.as_str(), ok.username.as_str()),
            ("http://proxy.corp:3128", "alifu")
        );
    }

    #[test]
    fn custom_needs_a_url_and_other_modes_do_not() {
        assert!(validate(&input(ProxyMode::Custom, "", &[])).is_err());
        assert!(validate(&input(ProxyMode::None, "", &[])).is_ok());
    }

    #[test]
    fn bypass_patterns_are_trimmed_capped_and_checked() {
        let many: Vec<String> = (0..=MAX_BYPASS).map(|i| format!("h{i}")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();

        let kept = validate(&input(ProxyMode::None, "", &[" a ", "", "*.internal"])).unwrap();

        assert_eq!(kept.bypass, ["a", "*.internal"]);
        assert!(validate(&input(ProxyMode::None, "", &many)).is_err());
        assert!(validate(&input(ProxyMode::None, "", &["a,b"])).is_err());
    }

    #[test]
    fn a_stored_proxy_is_valid_only_in_its_normal_form() {
        let good = validate(&input(ProxyMode::Custom, "http://p:1", &[])).unwrap();
        let bad = StoredProxy {
            url: "http://u:pw@p:1".to_string(),
            ..good.clone()
        };

        assert!(is_valid(&good));
        assert!(!is_valid(&bad));
        assert_eq!(host_of(&good), "p");
    }
}
