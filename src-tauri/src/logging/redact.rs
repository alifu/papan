//! Redaction helpers. Use these before anything user-supplied reaches a log line.

use std::borrow::Cow;

/// Replacement text written in place of a sensitive value.
pub const REDACTED: &str = "[REDACTED]";

/// Header names (lowercase) whose values must never be logged.
const SENSITIVE_HEADERS: [&str; 7] = [
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "x-auth-token",
    "x-csrf-token",
];

/// Whether a header's value must be hidden. Header names are case-insensitive.
pub fn is_sensitive_header(name: &str) -> bool {
    let name = name.trim();
    SENSITIVE_HEADERS
        .iter()
        .any(|sensitive| name.eq_ignore_ascii_case(sensitive))
}

/// Returns the header value to log: the original, or [`REDACTED`] for sensitive headers.
pub fn redact_header_value<'a>(name: &str, value: &'a str) -> Cow<'a, str> {
    if is_sensitive_header(name) {
        Cow::Borrowed(REDACTED)
    } else {
        Cow::Borrowed(value)
    }
}

/// Returns `text` with every occurrence of each secret replaced by [`REDACTED`].
///
/// Longer secrets are replaced first so a secret containing another is fully hidden.
/// Empty secrets are ignored.
pub fn redact_secrets(text: &str, secrets: &[&str]) -> String {
    let mut ordered: Vec<&str> = secrets.iter().copied().filter(|s| !s.is_empty()).collect();
    ordered.sort_by_key(|secret| std::cmp::Reverse(secret.len()));

    ordered.iter().fold(text.to_owned(), |redacted, secret| {
        redacted.replace(secret, REDACTED)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_headers_are_detected_case_insensitively() {
        for name in ["Authorization", "COOKIE", "x-api-key", " Set-Cookie "] {
            assert!(is_sensitive_header(name), "{name} should be sensitive");
        }
    }

    #[test]
    fn ordinary_headers_are_not_sensitive() {
        for name in ["Content-Type", "Accept", "X-Request-Id", ""] {
            assert!(!is_sensitive_header(name), "{name} should not be sensitive");
        }
    }

    #[test]
    fn sensitive_header_value_is_replaced() {
        let value = redact_header_value("Authorization", "Bearer abc123");

        assert_eq!(value, REDACTED);
    }

    #[test]
    fn ordinary_header_value_is_kept() {
        let value = redact_header_value("Content-Type", "application/json");

        assert_eq!(value, "application/json");
    }

    #[test]
    fn secrets_are_replaced_everywhere_in_text() {
        let text = "token=abc123&again=abc123";

        let redacted = redact_secrets(text, &["abc123"]);

        assert_eq!(redacted, "token=[REDACTED]&again=[REDACTED]");
    }

    #[test]
    fn longer_secret_is_hidden_before_a_secret_it_contains() {
        let redacted = redact_secrets("key=abc123xyz", &["abc", "abc123xyz"]);

        assert_eq!(redacted, "key=[REDACTED]");
    }

    #[test]
    fn empty_secrets_are_ignored() {
        let redacted = redact_secrets("nothing to hide", &["", ""]);

        assert_eq!(redacted, "nothing to hide");
    }
}
