//! The cookie manager over one `CookieStore` (plan 03b §6.5): list, set, delete.
//! Values are shown (the user already sees them in every response) but never logged.

use cookie_store::CookieExpiration;
use reqwest_cookie_store::{CookieStore, RawCookie};
use time::OffsetDateTime;
use url::Url;

use super::identity::normalize_host;
use super::types::{CookieFilter, CookieInput, StoredCookie};
use crate::error::AppError;

const NANOS_PER_MS: f64 = 1_000_000.0;

fn invalid(message: &str) -> AppError {
    AppError::invalid_input(message)
}

/// The unexpired cookies of `store`, sorted by domain, path, name.
pub fn list(store: &CookieStore) -> Vec<StoredCookie> {
    let mut cookies: Vec<StoredCookie> = store
        .iter_unexpired()
        .map(|cookie| StoredCookie {
            name: cookie.name().to_string(),
            value: cookie.value().to_string(),
            domain: String::from(&cookie.domain),
            path: String::from(&cookie.path),
            expires_at_ms: match cookie.expires {
                CookieExpiration::AtUtc(at) => {
                    Some(at.unix_timestamp_nanos() as f64 / NANOS_PER_MS)
                }
                CookieExpiration::SessionEnd => None,
            },
            http_only: cookie.http_only().unwrap_or(false),
            secure: cookie.secure().unwrap_or(false),
        })
        .collect();
    cookies.sort_by(|a, b| (&a.domain, &a.path, &a.name).cmp(&(&b.domain, &b.path, &b.name)));
    cookies
}

/// Neither a control character nor `;` or `,`.
fn is_plain(text: &str) -> bool {
    !text.chars().any(|c| c.is_control() || c == ';' || c == ',')
}

fn expiry(expires_at_ms: Option<f64>) -> Result<Option<OffsetDateTime>, AppError> {
    let Some(ms) = expires_at_ms else {
        return Ok(None);
    };
    let at = ms
        .is_finite()
        .then(|| OffsetDateTime::from_unix_timestamp_nanos((ms * NANOS_PER_MS) as i128).ok())
        .flatten()
        .ok_or_else(|| invalid("The expiry date is not valid."))?;
    if at <= OffsetDateTime::now_utc() {
        return Err(invalid("The expiry date is in the past."));
    }
    Ok(Some(at))
}

/// Adds `input`, or replaces the cookie with its domain, path and name. The cookie is
/// host-only: it is sent to `domain` itself.
pub fn set(store: &mut CookieStore, input: &CookieInput) -> Result<(), AppError> {
    if input.name.is_empty() || !is_plain(&input.name) || input.name.contains(['=', ' ']) {
        return Err(invalid(
            "Enter a cookie name without spaces, control characters, `=`, `;` or `,`.",
        ));
    }
    if !is_plain(&input.value) {
        return Err(invalid(
            "Enter a cookie value without control characters, `;` or `,`.",
        ));
    }
    if !input.path.starts_with('/') || !is_plain(&input.path) {
        return Err(invalid("Enter a cookie path that starts with `/`."));
    }
    let domain = normalize_host(&input.domain)
        .map_err(|_| invalid("Enter the cookie domain as a host name, without scheme or port."))?;
    let url = Url::parse(&format!("https://{domain}{}", input.path))
        .map_err(|_| invalid("Enter a cookie path that starts with `/`."))?;
    let builder = RawCookie::build((input.name.clone(), input.value.clone()))
        .path(input.path.clone())
        .http_only(input.http_only)
        .secure(input.secure);
    let raw = match expiry(input.expires_at_ms)? {
        Some(at) => builder.expires(at).build(),
        None => builder.build(),
    };
    store
        .insert_raw(&raw, &url)
        .map(|_| ())
        .map_err(|_| invalid("The cookie could not be stored."))
}

/// Deletes every cookie of `filter.domain`, or the one named by domain + path + name.
/// Returns how many were removed.
pub fn delete(store: &mut CookieStore, filter: &CookieFilter) -> Result<usize, AppError> {
    let domain = filter.domain.trim().to_ascii_lowercase();
    let targets: Vec<(String, String, String)> = match (&filter.path, &filter.name) {
        (None, None) => store
            .iter_any()
            .filter(|cookie| String::from(&cookie.domain) == domain)
            .map(|cookie| {
                let path = String::from(&cookie.path);
                (domain.clone(), path, cookie.name().to_string())
            })
            .collect(),
        (Some(path), Some(name)) => vec![(domain, path.clone(), name.clone())],
        _ => {
            return Err(invalid(
                "Name both the path and the name of the cookie, or neither.",
            ))
        }
    };
    Ok(targets
        .iter()
        .filter(|(domain, path, name)| store.remove(domain, path, name).is_some())
        .count())
}

#[cfg(test)]
#[path = "cookies_tests.rs"]
mod tests;
