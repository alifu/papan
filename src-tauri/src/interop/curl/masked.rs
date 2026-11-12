//! What Copy as cURL exports (plan 03b §7): the draft with plain variables resolved in
//! M4's mask mode, except that a secret is left as typed (`{{api_token}}`). The vault is
//! never opened: secrets enter the resolver without a value, so none can be exported.

use super::export::export;
use crate::error::AppError;
use crate::store::types::{Auth, Body, KeyValue, Part, RequestContent, RequestLocation};
use crate::store::Store;
use crate::vars::resolve::{mask_text, Report};
use crate::vars::scopes::{Layer, Scopes, Value};
use crate::vars::service::Snapshot;
use crate::vars::VarsService;

/// The cURL command for `content` at `location`. Reads variable files, never the vault.
pub fn export_masked(
    service: &VarsService,
    store: &Store,
    location: &RequestLocation,
    content: &RequestContent,
) -> Result<String, AppError> {
    let scopes = scopes_without_secrets(&service.snapshot(store, location));
    export(&masked_content(content, &scopes))
}

/// Resolver layers in which every secret has no value.
pub fn scopes_without_secrets(snapshot: &Snapshot) -> Scopes {
    let layers = snapshot
        .scopes
        .iter()
        .map(|data| Layer {
            source: data.source,
            vars: data
                .table
                .iter()
                .filter(|(_, entry)| entry.enabled)
                .map(|(name, entry)| {
                    let value = if entry.secret {
                        Value::secret(None)
                    } else {
                        Value::plain(data.current.get(name).unwrap_or(&entry.value).clone())
                    };
                    (name.clone(), value)
                })
                .collect(),
        })
        .collect();
    Scopes::new(layers)
}

/// `content` with every field the sender resolves shown in mask mode.
pub fn masked_content(content: &RequestContent, scopes: &Scopes) -> RequestContent {
    let text = |value: &str| mask_text(value, scopes, &mut Report::default());
    let row = |row: &KeyValue| KeyValue {
        name: text(&row.name),
        value: text(&row.value),
        enabled: row.enabled,
    };
    RequestContent {
        url: text(&content.url),
        headers: content.headers.iter().map(row).collect(),
        body: masked_body(&content.body, &text),
        auth: masked_auth(&content.auth, &text),
        ..content.clone()
    }
}

fn masked_body(body: &Body, text: &dyn Fn(&str) -> String) -> Body {
    match body {
        Body::Json { text: value } => Body::Json { text: text(value) },
        Body::Xml { text: value } => Body::Xml { text: text(value) },
        Body::Text { text: value } => Body::Text { text: text(value) },
        Body::Form { fields } => Body::Form {
            fields: fields
                .iter()
                .map(|field| KeyValue {
                    name: text(&field.name),
                    value: text(&field.value),
                    enabled: field.enabled,
                })
                .collect(),
        },
        Body::Multipart { parts } => Body::Multipart {
            parts: parts.iter().map(|part| masked_part(part, text)).collect(),
        },
        Body::None | Body::Binary { .. } | Body::Unsupported => body.clone(),
    }
}

fn masked_part(part: &Part, text: &dyn Fn(&str) -> String) -> Part {
    let is_file = part.file.as_deref().is_some_and(|file| !file.is_empty());
    Part {
        name: text(&part.name),
        value: if is_file {
            part.value.clone()
        } else {
            text(&part.value)
        },
        ..part.clone()
    }
}

/// OAuth 2 settings are not exported (the token becomes a placeholder), so they stay as
/// typed.
fn masked_auth(auth: &Auth, text: &dyn Fn(&str) -> String) -> Auth {
    match auth {
        Auth::Basic { username, password } => Auth::Basic {
            username: text(username),
            password: text(password),
        },
        Auth::Digest { username, password } => Auth::Digest {
            username: text(username),
            password: text(password),
        },
        Auth::Bearer { token } => Auth::Bearer { token: text(token) },
        Auth::ApiKey {
            name,
            value,
            placement,
        } => Auth::ApiKey {
            name: text(name),
            value: text(value),
            placement: *placement,
        },
        Auth::Oauth2 { .. } | Auth::None | Auth::Unsupported => auth.clone(),
    }
}
