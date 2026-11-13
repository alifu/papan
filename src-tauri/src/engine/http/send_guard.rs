//! Guards around one send (plan 03b §5.3, §6.3, §15.2): which values are masked in the
//! shown URL, logs and errors, which headers count as secret, and the refusal of a URL
//! whose host is still an unresolved variable.

use std::borrow::Cow;

use reqwest::header::HeaderName;

use super::build::{Prepared, INVALID_URL_MESSAGE};
use super::resolve::Resolved;
use crate::error::{AppError, ErrorCode};
use crate::store::types::Auth;
use crate::vars::template;

/// The Digest password is masked like a secret value (plan 03b §6.3).
pub(super) fn with_digest_secret(resolved: &Resolved) -> Cow<'_, Resolved> {
    match &resolved.content.auth {
        Auth::Digest { password, .. } => with_secret(resolved, Some(password)),
        _ => Cow::Borrowed(resolved),
    }
}

/// `resolved` with `secret` added to the values masked in the URL and errors; the proxy
/// password goes through here for every send (§15.2).
pub(super) fn with_secret<'a>(resolved: &'a Resolved, secret: Option<&str>) -> Cow<'a, Resolved> {
    match secret {
        Some(secret)
            if !secret.is_empty() && !resolved.secret_values.iter().any(|s| s == secret) =>
        {
            Cow::Owned(Resolved {
                secret_values: resolved
                    .secret_values
                    .iter()
                    .cloned()
                    .chain([secret.to_string()])
                    .collect(),
                ..resolved.clone()
            })
        }
        _ => Cow::Borrowed(resolved),
    }
}

pub(super) fn with_secret_headers(prepared: Prepared, resolved: &Resolved) -> Prepared {
    let extra = resolved
        .secret_headers
        .iter()
        .filter_map(|name| HeaderName::from_bytes(name.as_bytes()).ok());
    Prepared {
        secret_headers: prepared.secret_headers.into_iter().chain(extra).collect(),
        ..prepared
    }
}

fn not_defined(resolved: &Resolved) -> AppError {
    let names: Vec<String> = resolved
        .unresolved
        .iter()
        .map(|n| format!("`{n}`"))
        .collect();
    AppError::invalid_input(format!(
        "Not defined: {}. Select an environment or define it.",
        names.join(", ")
    ))
}

/// An unresolved token left in the scheme or host: the URL cannot be meant as typed.
pub(super) fn undefined_in_host(resolved: &Resolved) -> Option<AppError> {
    let url = resolved.content.url.trim();
    let without_scheme = ["https://", "http://"]
        .iter()
        .find_map(|scheme| {
            let head = url.get(..scheme.len())?;
            head.eq_ignore_ascii_case(scheme)
                .then(|| &url[scheme.len()..])
        })
        .unwrap_or(url);
    let authority = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let broken = template::names(authority)
        .iter()
        .any(|name| resolved.unresolved.iter().any(|missing| missing == name));
    broken.then(|| not_defined(resolved))
}

/// Masks secrets in a failed send's message and detail. An invalid URL that still has
/// unresolved variables names them instead.
pub(super) fn redact_error(error: AppError, resolved: &Resolved) -> AppError {
    if error.code == ErrorCode::InvalidInput
        && error.message == INVALID_URL_MESSAGE
        && !resolved.unresolved.is_empty()
    {
        let names: Vec<String> = resolved
            .unresolved
            .iter()
            .map(|n| format!("`{n}`"))
            .collect();
        return AppError::invalid_input(format!(
            "Not defined: {}. Select an environment or define it.",
            names.join(", ")
        ));
    }
    AppError {
        message: resolved.redact(&error.message),
        detail: error
            .detail
            .as_deref()
            .map(|detail| resolved.redact(detail)),
        ..error
    }
}
