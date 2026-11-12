//! Literal secrets of collection requests are kept in memory only (plan 03 §4).
//!
//! A collection file gets [`redact`]`(content)`; the full content is remembered for the
//! session and handed back while the file on disk still matches its redacted form.

use std::collections::HashMap;

use super::types::{Auth, KeyValue, RequestContent, RequestLocation};
use crate::logging::redact::is_sensitive_header;

/// Whether `value` is exactly one `{{name}}` reference (surrounding spaces allowed).
fn is_variable(value: &str) -> bool {
    value
        .trim()
        .strip_prefix("{{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .is_some_and(|name| !name.trim().is_empty() && !name.contains('{') && !name.contains('}'))
}

/// A value that is entirely a `{{var}}` reference is not a secret and is kept; any other
/// value is blanked.
fn blank(value: &str) -> String {
    if is_variable(value) {
        value.to_string()
    } else {
        String::new()
    }
}

fn redact_header(header: &KeyValue) -> KeyValue {
    if is_sensitive_header(&header.name) {
        KeyValue {
            value: blank(&header.value),
            ..header.clone()
        }
    } else {
        header.clone()
    }
}

fn redact_auth(auth: &Auth) -> Auth {
    match auth {
        Auth::Basic { username, password } => Auth::Basic {
            username: username.clone(),
            password: blank(password),
        },
        Auth::Bearer { token } => Auth::Bearer {
            token: blank(token),
        },
        Auth::ApiKey {
            name,
            value,
            placement,
        } => Auth::ApiKey {
            name: name.clone(),
            value: blank(value),
            placement: *placement,
        },
        Auth::None | Auth::Unsupported => auth.clone(),
    }
}

/// A copy with every literal secret (password, token, API key, sensitive header value)
/// set to `""`.
pub fn redact(content: &RequestContent) -> RequestContent {
    RequestContent {
        headers: content.headers.iter().map(redact_header).collect(),
        auth: redact_auth(&content.auth),
        ..content.clone()
    }
}

/// The full content of collection requests written in this session, by location.
#[derive(Debug, Default)]
pub struct SessionSecrets {
    remembered: HashMap<RequestLocation, RequestContent>,
}

impl SessionSecrets {
    /// Remembers `full` (as written, normalised) when it holds a secret; forgets otherwise.
    pub fn remember(&mut self, location: &RequestLocation, full: RequestContent) {
        if redact(&full) == full {
            self.remembered.remove(location);
        } else {
            self.remembered.insert(location.clone(), full);
        }
    }

    /// The remembered content while `disk` is still its redacted form; otherwise `disk`
    /// (the file changed outside the app) and the remembered copy is dropped.
    pub fn recall(&mut self, location: &RequestLocation, disk: RequestContent) -> RequestContent {
        match self.remembered.get(location) {
            Some(full) if redact(full) == disk => full.clone(),
            Some(_) => {
                self.remembered.remove(location);
                disk
            }
            None => disk,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::types::ApiKeyPlacement;

    fn header(name: &str, value: &str) -> KeyValue {
        KeyValue {
            name: name.into(),
            value: value.into(),
            enabled: true,
        }
    }

    fn with_auth(auth: Auth) -> RequestContent {
        RequestContent {
            auth,
            ..RequestContent::new("GET", "https://x")
        }
    }

    #[test]
    fn redact_blanks_each_secret_field_and_keeps_variables() {
        let cases = [
            (
                Auth::Basic {
                    username: "u".into(),
                    password: "p".into(),
                },
                Auth::Basic {
                    username: "u".into(),
                    password: String::new(),
                },
            ),
            (
                Auth::Bearer { token: "t".into() },
                Auth::Bearer {
                    token: String::new(),
                },
            ),
            (
                Auth::Bearer {
                    token: "{{token}}".into(),
                },
                Auth::Bearer {
                    token: "{{token}}".into(),
                },
            ),
            (
                Auth::ApiKey {
                    name: "k".into(),
                    value: "v".into(),
                    placement: ApiKeyPlacement::Query,
                },
                Auth::ApiKey {
                    name: "k".into(),
                    value: String::new(),
                    placement: ApiKeyPlacement::Query,
                },
            ),
            (Auth::Unsupported, Auth::Unsupported),
        ];
        for (auth, expected) in cases {
            assert_eq!(redact(&with_auth(auth)).auth, expected);
        }
    }

    #[test]
    fn only_a_whole_variable_reference_is_kept() {
        for (value, kept) in [
            ("{{token}}", true),
            (" {{ token }} ", true),
            ("abc{{x", false),
            ("Bearer {{token}}", false),
            ("{{a}}{{b}}", false),
            ("{{}}", false),
            ("secret", false),
        ] {
            assert_eq!(blank(value) == value, kept, "{value}");
        }
    }

    #[test]
    fn redact_blanks_sensitive_headers_in_any_case() {
        let content = RequestContent {
            headers: vec![
                header("AUTHORIZATION", "Bearer x"),
                header("cookie", "a=b"),
                header("X-Api-Key", "{{key}}"),
                header("Accept", "json"),
            ],
            ..RequestContent::default()
        };

        let values: Vec<String> = redact(&content)
            .headers
            .into_iter()
            .map(|h| h.value)
            .collect();

        assert_eq!(values, ["", "", "{{key}}", "json"]);
    }

    #[test]
    fn recall_returns_the_remembered_copy_until_the_disk_changes() {
        let location = RequestLocation::Scratch { id: "a".into() };
        let full = with_auth(Auth::Bearer { token: "t".into() });
        let mut secrets = SessionSecrets::default();
        secrets.remember(&location, full.clone());

        let matching = secrets.recall(&location, redact(&full));
        let changed = RequestContent {
            url: "https://other".into(),
            ..redact(&full)
        };
        let after_change = secrets.recall(&location, changed.clone());
        let later = secrets.recall(&location, redact(&full));

        assert_eq!(matching, full);
        assert_eq!(after_change, changed);
        assert_eq!(later, redact(&full));
    }

    #[test]
    fn content_without_secrets_is_not_remembered() {
        let location = RequestLocation::Scratch { id: "a".into() };
        let mut secrets = SessionSecrets::default();
        secrets.remember(&location, with_auth(Auth::Bearer { token: "t".into() }));

        secrets.remember(&location, RequestContent::new("GET", "x"));

        assert!(secrets.remembered.is_empty());
    }
}
