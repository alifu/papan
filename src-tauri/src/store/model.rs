//! The three file schemas (plan §3.2), their validation and canonical serialisation.
//! Unknown fields are kept in `extra` and written back unchanged (plan §3.3).

use std::collections::BTreeMap;

use serde::Deserialize;

use super::types::RequestContent;
use super::yaml::{self, Node};
use super::{request_emit, request_model};
use crate::error::AppError;

pub const SUPPORTED_VERSION: u64 = 1;
const MAX_NAME_CHARS: usize = 100;
const MAX_METHOD_CHARS: usize = 20;
const MAX_URL_CHARS: usize = 8192;
const REQUEST_KIND: &str = "http";
/// Label used in messages about request content that has no file name yet.
const CONTENT_LABEL: &str = "Request";

type Extra = BTreeMap<String, serde_json::Value>;

/// `requests/**/<slug>.http.yaml` and `scratch/<uuid>.http.yaml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RequestFile {
    pub kind: String,
    pub method: String,
    pub name: String,
    #[serde(default)]
    pub url: String,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `_folder.yaml`: optional display name and child order.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct FolderFile {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub order: Vec<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `papan.collection.yaml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CollectionFile {
    pub name: String,
    pub version: u64,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Checks a display name as stored; user input is trimmed first by [`clean_name`].
pub fn check_name(label: &str, name: &str) -> Result<(), AppError> {
    let length = name.trim().chars().count();
    if length == 0 || length > MAX_NAME_CHARS {
        return Err(yaml::invalid(
            label,
            "field `name` must be 1–100 characters.",
        ));
    }
    if name.chars().any(char::is_control) {
        return Err(yaml::invalid(
            label,
            "field `name` must not contain control characters.",
        ));
    }
    Ok(())
}

/// Trims and validates a name typed by the user.
pub fn clean_name(name: &str) -> Result<String, AppError> {
    let trimmed = name.trim();
    check_name("Name", trimmed)?;
    Ok(trimmed.to_string())
}

pub fn check_method(label: &str, method: &str) -> Result<(), AppError> {
    let length = method.chars().count();
    let letters_only = method.chars().all(|c| c.is_ascii_uppercase());
    if length == 0 || length > MAX_METHOD_CHARS || !letters_only {
        return Err(yaml::invalid(
            label,
            "field `method` must be 1–20 capital letters (A–Z).",
        ));
    }
    Ok(())
}

pub fn check_url(label: &str, url: &str) -> Result<(), AppError> {
    if url.chars().count() > MAX_URL_CHARS {
        return Err(yaml::invalid(
            label,
            "field `url` is longer than 8192 characters.",
        ));
    }
    Ok(())
}

fn extra_nodes(extra: &Extra) -> BTreeMap<String, Node> {
    extra
        .iter()
        .map(|(key, value)| (key.clone(), Node::from(value)))
        .collect()
}

fn emit_with(extra: &Extra, known: Vec<(&str, Node)>) -> Result<String, AppError> {
    let map = extra_nodes(extra)
        .into_iter()
        .chain(known.into_iter().map(|(key, node)| (key.to_string(), node)))
        .collect();
    yaml::emit(&map)
}

impl RequestFile {
    pub fn new(name: String, method: &str) -> Self {
        Self {
            kind: REQUEST_KIND.to_string(),
            method: method.to_string(),
            name,
            url: String::new(),
            extra: Extra::new(),
        }
    }

    pub fn parse(label: &str, bytes: &[u8]) -> Result<Self, AppError> {
        let file: Self = yaml::parse(label, bytes)?;
        file.validate(label)?;
        Ok(file)
    }

    pub fn validate(&self, label: &str) -> Result<(), AppError> {
        if self.kind != REQUEST_KIND {
            return Err(yaml::invalid(label, "field `kind` must be `http`."));
        }
        check_name(label, &self.name)?;
        check_method(label, &self.method)?;
        check_url(label, &self.url)?;
        request_model::parse_sections(label, &self.extra).map(|_| ())
    }

    /// The typed request content (plan 03 §3).
    pub fn content(&self) -> Result<RequestContent, AppError> {
        let sections = request_model::parse_sections(CONTENT_LABEL, &self.extra)?;
        Ok(RequestContent {
            method: self.method.clone(),
            url: self.url.clone(),
            ..sections
        })
    }

    pub fn to_yaml(&self) -> Result<String, AppError> {
        let extra = request_emit::canonical_extra(CONTENT_LABEL, &self.extra)?;
        let url = (!self.url.is_empty()).then(|| ("url", Node::String(self.url.clone())));
        let known = [
            ("kind", Node::String(self.kind.clone())),
            ("method", Node::String(self.method.clone())),
            ("name", Node::String(self.name.clone())),
        ]
        .into_iter()
        .chain(url)
        .collect();
        emit_with(&extra, known)
    }

    /// Copy holding `content` (validated first); name and unknown fields are kept, and
    /// so is the on-disk form of every section that `content` leaves unchanged.
    pub fn with_content(&self, content: &RequestContent) -> Result<Self, AppError> {
        request_model::validate_content(CONTENT_LABEL, content)?;
        Ok(Self {
            method: content.method.clone(),
            url: content.url.clone(),
            extra: request_emit::merge(CONTENT_LABEL, &self.extra, content)?,
            ..self.clone()
        })
    }

    #[must_use]
    pub fn with_name(&self, name: &str) -> Self {
        Self {
            name: name.to_string(),
            ..self.clone()
        }
    }
}

impl FolderFile {
    pub fn parse(label: &str, bytes: &[u8]) -> Result<Self, AppError> {
        let file: Self = yaml::parse(label, bytes)?;
        if let Some(name) = &file.name {
            check_name(label, name)?;
        }
        Ok(file)
    }

    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.order.is_empty() && self.extra.is_empty()
    }

    pub fn to_yaml(&self) -> Result<String, AppError> {
        let name = self
            .name
            .as_ref()
            .map(|name| ("name", Node::String(name.clone())));
        let order = (!self.order.is_empty()).then(|| {
            let items = self.order.iter().cloned().map(Node::String).collect();
            ("order", Node::Seq(items))
        });
        emit_with(&self.extra, name.into_iter().chain(order).collect())
    }

    #[must_use]
    pub fn with_order(&self, order: Vec<String>) -> Self {
        Self {
            order,
            ..self.clone()
        }
    }

    #[must_use]
    pub fn with_name(&self, name: &str) -> Self {
        Self {
            name: Some(name.to_string()),
            ..self.clone()
        }
    }
}

impl CollectionFile {
    pub fn new(name: String) -> Self {
        Self {
            name,
            version: SUPPORTED_VERSION,
            extra: Extra::new(),
        }
    }

    pub fn parse(label: &str, bytes: &[u8]) -> Result<Self, AppError> {
        let file: Self = yaml::parse(label, bytes)?;
        check_name(label, &file.name)?;
        if file.version > SUPPORTED_VERSION {
            return Err(AppError::invalid_input(
                "This collection was made by a newer Papan.",
            ));
        }
        if file.version < SUPPORTED_VERSION {
            return Err(yaml::invalid(label, "field `version` must be 1."));
        }
        Ok(file)
    }

    pub fn to_yaml(&self) -> Result<String, AppError> {
        let known = vec![
            ("name", Node::String(self.name.clone())),
            ("version", Node::Number(self.version.into())),
        ];
        emit_with(&self.extra, known)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trips_and_omits_empty_url() {
        let file = RequestFile::new("Ping".into(), "GET");

        let text = file.to_yaml().unwrap();

        assert_eq!(text, "kind: http\nmethod: GET\nname: Ping\n");
        assert_eq!(RequestFile::parse("p", text.as_bytes()).unwrap(), file);
    }

    #[test]
    fn request_validation_names_the_field() {
        let base = RequestFile::new("Ok".into(), "GET");
        let with = |method: &str, url: &str| RequestFile {
            method: method.into(),
            url: url.into(),
            ..base.clone()
        };
        let cases = [
            (with("get", ""), "method"),
            (with("ABCDEFGHIJKLMNOPQRSTU", ""), "method"),
            (with("GET", &"u".repeat(8193)), "url"),
            (base.with_name("   "), "name"),
            (base.with_name("a\u{7}"), "name"),
            (
                RequestFile {
                    kind: "graphql".into(),
                    ..base.clone()
                },
                "kind",
            ),
        ];
        for (file, field) in cases {
            let error = file.validate("r.http.yaml").unwrap_err();
            assert!(error.message.contains(field), "{}", error.message);
        }
    }

    #[test]
    fn clean_name_trims_and_rejects_blank() {
        assert_eq!(clean_name("  Users ").unwrap(), "Users");
        assert!(clean_name(" ").is_err());
    }

    #[test]
    fn folder_file_is_optional_name_and_order() {
        let file = FolderFile::parse("_folder.yaml", b"order:\n  - a\n").unwrap();

        assert_eq!(file.name, None);
        assert_eq!(file.to_yaml().unwrap(), "order:\n  - a\n");
        assert!(FolderFile::default().is_empty());
        assert_eq!(
            FolderFile::default().with_name("Auth").to_yaml().unwrap(),
            "name: Auth\n"
        );
    }

    #[test]
    fn collection_version_rules() {
        let newer = CollectionFile::parse("c", b"name: A\nversion: 2\n").unwrap_err();
        let older = CollectionFile::parse("c", b"name: A\nversion: 0\n").unwrap_err();
        let ok = CollectionFile::parse("c", b"name: A\nversion: 1\n").unwrap();

        assert_eq!(newer.message, "This collection was made by a newer Papan.");
        assert!(older.message.contains("version"));
        assert_eq!(ok.to_yaml().unwrap(), "name: A\nversion: 1\n");
        assert_eq!(CollectionFile::new("A".into()), ok);
    }
}
