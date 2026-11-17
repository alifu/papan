//! Secret handling on import (plan 09 §3 "Secrets"): values that must never reach YAML
//! are held in [`SecretValues`] (redacting `Debug`), and literal credentials in requests
//! are lifted into collection secret variables `<request-slug>_<field>`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::logging::redact::is_sensitive_header;
use crate::store::types::{Auth, Body, KeyValue, RequestContent};
use crate::vars::template::is_single_reference;

const HEADER_SCHEMES: [&str; 3] = ["bearer ", "basic ", "token "];
const SENSITIVE_NAME_PARTS: [&str; 6] =
    ["token", "secret", "password", "apikey", "api_key", "auth"];

/// Secret values by variable name. `Debug` prints names only.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SecretValues(BTreeMap<String, String>);

impl fmt::Debug for SecretValues {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set()
            .entries(self.0.keys().map(|name| format!("{name}=<redacted>")))
            .finish()
    }
}

impl SecretValues {
    pub fn insert(&mut self, name: &str, value: &str) {
        self.0.insert(name.to_string(), value.to_string());
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }

    pub fn remove(&mut self, name: &str) -> Option<String> {
        self.0.remove(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Whether a variable name looks like it holds a credential (pre-ticked in the preview).
pub fn looks_sensitive(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SENSITIVE_NAME_PARTS.iter().any(|part| lower.contains(part))
}

/// A value that may stay in a file: empty or exactly one `{{ref}}`.
fn is_reference(value: &str) -> bool {
    value.is_empty() || is_single_reference(value.trim())
}

/// A sensitive header value that may stay: a reference, optionally after a scheme word.
fn is_header_reference(value: &str) -> bool {
    let trimmed = value.trim();
    let after_scheme = HEADER_SCHEMES.iter().find_map(|scheme| {
        let head = trimmed.get(..scheme.len())?;
        head.eq_ignore_ascii_case(scheme)
            .then(|| &trimmed[scheme.len()..])
    });
    is_reference(trimmed) || after_scheme.is_some_and(is_single_reference)
}

/// Moves literal credentials into secret variables, one variable per distinct literal.
#[derive(Debug)]
pub struct Lifter {
    taken: BTreeSet<String>,
    by_value: BTreeMap<String, String>,
    values: SecretValues,
    room: usize,
}

impl Lifter {
    /// `taken` are names already used in the collection scope; `room` is how many more
    /// variables the scope can hold.
    pub fn new(taken: impl IntoIterator<Item = String>, room: usize) -> Self {
        Self {
            taken: taken.into_iter().collect(),
            by_value: BTreeMap::new(),
            values: SecretValues::default(),
            room,
        }
    }

    /// `content` with every literal credential replaced by `{{name}}`, and report notes.
    pub fn lift(&mut self, slug: &str, content: &RequestContent) -> (RequestContent, Vec<String>) {
        let mut notes = Vec::new();
        let base = slug.replace('-', "_");
        let headers = content
            .headers
            .iter()
            .map(|header| self.lift_header(&base, header, &mut notes))
            .collect();
        let auth = self.lift_auth(&base, &content.auth, &mut notes);
        let url = self.lift_url(&base, &content.url, &mut notes);
        let body = self.lift_form(&base, &content.body, &mut notes);
        let lifted = RequestContent {
            headers,
            auth,
            url,
            body,
            ..content.clone()
        };
        (lifted, notes)
    }

    /// `user:pass@` passwords and sensitive query values (`token`, `api_key`, …) in `url`.
    fn lift_url(&mut self, base: &str, url: &str, notes: &mut Vec<String>) -> String {
        let (head, fragment) = match url.split_once('#') {
            Some((head, fragment)) => (head, Some(fragment)),
            None => (url, None),
        };
        let (path, query) = match head.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (head, None),
        };
        let mut out = self.lift_userinfo(base, path, notes);
        if let Some(query) = query {
            let pairs: Vec<String> = query
                .split('&')
                .map(|pair| match pair.split_once('=') {
                    Some((name, value)) if looks_sensitive(name) && !is_reference(value) => {
                        let field = name.to_ascii_lowercase().replace('-', "_");
                        format!("{name}={}", self.field(base, &field, value, notes))
                    }
                    _ => pair.to_string(),
                })
                .collect();
            out = format!("{out}?{}", pairs.join("&"));
        }
        if let Some(fragment) = fragment {
            out = format!("{out}#{fragment}");
        }
        out
    }

    fn lift_userinfo(&mut self, base: &str, path: &str, notes: &mut Vec<String>) -> String {
        let Some((scheme, rest)) = path.split_once("://") else {
            return path.to_string();
        };
        let (authority, tail) = match rest.find('/') {
            Some(at) => rest.split_at(at),
            None => (rest, ""),
        };
        let Some((userinfo, host)) = authority.rsplit_once('@') else {
            return path.to_string();
        };
        match userinfo.split_once(':') {
            Some((user, password)) if !is_reference(password) => {
                let name = self.field(base, "url_password", password, notes);
                format!("{scheme}://{user}:{name}@{host}{tail}")
            }
            _ => path.to_string(),
        }
    }

    /// Form fields whose names look sensitive (`password`, `client_secret`, …).
    fn lift_form(&mut self, base: &str, body: &Body, notes: &mut Vec<String>) -> Body {
        let Body::Form { fields } = body else {
            return body.clone();
        };
        let fields = fields
            .iter()
            .map(|field| {
                if !looks_sensitive(&field.name) || is_reference(&field.value) {
                    return field.clone();
                }
                let name = field.name.to_ascii_lowercase().replace('-', "_");
                KeyValue {
                    value: self.field(base, &name, &field.value, notes),
                    ..field.clone()
                }
            })
            .collect();
        Body::Form { fields }
    }

    pub fn into_values(self) -> SecretValues {
        self.values
    }

    fn lift_header(&mut self, base: &str, header: &KeyValue, notes: &mut Vec<String>) -> KeyValue {
        if !is_sensitive_header(&header.name) || is_header_reference(&header.value) {
            return header.clone();
        }
        let field = header.name.trim().to_ascii_lowercase().replace('-', "_");
        KeyValue {
            value: self.field(base, &field, &header.value, notes),
            ..header.clone()
        }
    }

    fn lift_auth(&mut self, base: &str, auth: &Auth, notes: &mut Vec<String>) -> Auth {
        let mut field = |name: &str, value: &str| {
            if is_reference(value) {
                value.to_string()
            } else {
                self.field(base, name, value, notes)
            }
        };
        match auth {
            Auth::Basic { username, password } => Auth::Basic {
                username: username.clone(),
                password: field("password", password),
            },
            Auth::Digest { username, password } => Auth::Digest {
                username: username.clone(),
                password: field("password", password),
            },
            Auth::Bearer { token } => Auth::Bearer {
                token: field("token", token),
            },
            Auth::ApiKey {
                name,
                value,
                placement,
            } => Auth::ApiKey {
                name: name.clone(),
                value: field("api_key", value),
                placement: *placement,
            },
            Auth::Oauth2 {
                grant,
                auth_url,
                token_url,
                client_id,
                client_secret,
                scope,
                client_auth,
                redirect_port,
            } => Auth::Oauth2 {
                grant: *grant,
                auth_url: auth_url.clone(),
                token_url: token_url.clone(),
                client_id: client_id.clone(),
                client_secret: field("client_secret", client_secret),
                scope: scope.clone(),
                client_auth: *client_auth,
                redirect_port: *redirect_port,
            },
            Auth::None | Auth::Unsupported => auth.clone(),
        }
    }

    /// The `{{name}}` that replaces `value` (reusing the name of an equal literal).
    fn field(&mut self, base: &str, field: &str, value: &str, notes: &mut Vec<String>) -> String {
        if let Some(name) = self.by_value.get(value) {
            return format!("{{{{{name}}}}}");
        }
        if self.room == 0 {
            notes.push(format!(
                "The literal `{field}` was removed: the collection has no room for another variable."
            ));
            return String::new();
        }
        let name = self.free_name(&format!("{base}_{field}"));
        self.room -= 1;
        self.taken.insert(name.clone());
        self.by_value.insert(value.to_string(), name.clone());
        self.values.insert(&name, value);
        notes.push(format!(
            "The literal `{field}` was moved to the secret variable `{name}`."
        ));
        format!("{{{{{name}}}}}")
    }

    fn free_name(&self, wanted: &str) -> String {
        std::iter::once(wanted.to_string())
            .chain((2..).map(|n| format!("{wanted}_{n}")))
            .find(|name| !self.taken.contains(name))
            .unwrap_or_else(|| wanted.to_string())
    }
}

#[cfg(test)]
#[path = "secrets_tests.rs"]
mod tests;
