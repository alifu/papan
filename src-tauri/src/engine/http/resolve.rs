//! `{{var}}` resolution of a request draft (plan 04 §6), the one exit through which
//! variable and secret values reach the engine. Method, settings, disabled rows and
//! file paths are never resolved: a variable must not choose which file is uploaded.

use super::mask::mask_secrets;
use crate::store::types::{Auth, Body, KeyValue, Part, RequestContent};
use crate::vars::dynamic::Dynamics;
use crate::vars::resolve::{resolve_text, Report};
use crate::vars::scopes::Scopes;

/// A draft with variables substituted, and what the substitution used.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resolved {
    pub content: RequestContent,
    pub unresolved: Vec<String>,
    /// For redaction of logs, errors and the shown URL.
    pub secret_values: Vec<String>,
    /// Names of user headers whose name or value used a secret.
    pub secret_headers: Vec<String>,
    /// The URL as typed, before resolution: what logs show.
    pub template_url: String,
}

impl Resolved {
    /// `content` sent as typed, with no variables.
    pub fn unchanged(content: RequestContent) -> Self {
        Self {
            template_url: content.url.clone(),
            content,
            ..Self::default()
        }
    }

    /// The URL as typed without its query or fragment, secrets masked: for log lines.
    pub fn loggable_url(&self) -> String {
        let without_query = self
            .template_url
            .split(['?', '#'])
            .next()
            .unwrap_or_default();
        self.redact(without_query.trim())
    }

    /// `text` with every secret masked, also percent-encoded or in another case.
    pub fn redact(&self, text: &str) -> String {
        let secrets: Vec<&str> = self.secret_values.iter().map(String::as_str).collect();
        mask_secrets(text, &secrets)
    }
}

struct Resolver<'a> {
    scopes: &'a Scopes,
    dynamics: &'a dyn Dynamics,
    report: Report,
    secret_headers: Vec<String>,
}

impl Resolver<'_> {
    fn text(&mut self, text: &str) -> String {
        self.tracked(text).0
    }

    /// Resolves `text` and says whether a secret went into it.
    fn tracked(&mut self, text: &str) -> (String, bool) {
        let mut report = Report::default();
        let resolved = resolve_text(text, self.scopes, self.dynamics, &mut report);
        let used = report.used_secret;
        self.report.unresolved.extend(report.unresolved);
        for secret in report.secret_values {
            if !self.report.secret_values.contains(&secret) {
                self.report.secret_values.push(secret);
            }
        }
        self.report.used_secret |= used;
        (resolved, used)
    }

    fn header(&mut self, row: &KeyValue) -> KeyValue {
        if !row.enabled {
            return row.clone();
        }
        let (name, secret_name) = self.tracked(&row.name);
        let (value, secret_value) = self.tracked(&row.value);
        if secret_name || secret_value {
            self.secret_headers.push(name.trim().to_string());
        }
        KeyValue {
            name,
            value,
            ..row.clone()
        }
    }

    fn field(&mut self, row: &KeyValue) -> KeyValue {
        if !row.enabled {
            return row.clone();
        }
        KeyValue {
            name: self.text(&row.name),
            value: self.text(&row.value),
            ..row.clone()
        }
    }

    fn part(&mut self, part: &Part) -> Part {
        if !part.enabled {
            return part.clone();
        }
        let is_file = part.file.as_deref().is_some_and(|file| !file.is_empty());
        Part {
            name: self.text(&part.name),
            value: if is_file {
                part.value.clone()
            } else {
                self.text(&part.value)
            },
            ..part.clone()
        }
    }

    fn body(&mut self, body: &Body) -> Body {
        match body {
            Body::Json { text } => Body::Json {
                text: self.text(text),
            },
            Body::Xml { text } => Body::Xml {
                text: self.text(text),
            },
            Body::Text { text } => Body::Text {
                text: self.text(text),
            },
            Body::Form { fields } => Body::Form {
                fields: fields.iter().map(|row| self.field(row)).collect(),
            },
            Body::Multipart { parts } => Body::Multipart {
                parts: parts.iter().map(|part| self.part(part)).collect(),
            },
            Body::None | Body::Binary { .. } | Body::Unsupported => body.clone(),
        }
    }

    fn auth(&mut self, auth: &Auth) -> Auth {
        match auth {
            Auth::Basic { username, password } => Auth::Basic {
                username: self.text(username),
                password: self.text(password),
            },
            Auth::Bearer { token } => Auth::Bearer {
                token: self.text(token),
            },
            Auth::ApiKey {
                name,
                value,
                placement,
            } => Auth::ApiKey {
                name: self.text(name),
                value: self.text(value),
                placement: *placement,
            },
            Auth::None | Auth::Unsupported => auth.clone(),
        }
    }
}

pub fn resolve_content(
    content: &RequestContent,
    scopes: &Scopes,
    dynamics: &dyn Dynamics,
) -> Resolved {
    let mut resolver = Resolver {
        scopes,
        dynamics,
        report: Report::default(),
        secret_headers: Vec::new(),
    };
    let resolved = RequestContent {
        url: resolver.text(&content.url),
        headers: content
            .headers
            .iter()
            .map(|row| resolver.header(row))
            .collect(),
        body: resolver.body(&content.body),
        auth: resolver.auth(&content.auth),
        ..content.clone()
    };
    Resolved {
        content: resolved,
        unresolved: resolver.report.unresolved.into_iter().collect(),
        secret_values: resolver.report.secret_values,
        secret_headers: resolver.secret_headers,
        template_url: content.url.clone(),
    }
}

#[cfg(test)]
#[path = "resolve_tests.rs"]
mod tests;
