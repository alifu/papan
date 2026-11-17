//! Serde types for Postman v2.0/v2.1 collections, environments and globals (plan 09 §9 B0).
//!
//! Reading goes through [`parse_collection`] / [`parse_variable_file`]: a path-aware walk over a
//! `serde_json::Value` that normalises Postman's polymorphic forms (string or object `url`,
//! v2.0 object auth vs v2.1 list auth, string or array `exec` …), counts unknown fields and
//! reports wrong types with their JSON path. Writing derives `Serialize` and always produces the
//! v2.1 shape with a fixed key order (field order below).

mod parse;
mod parse_parts;
mod read;
#[cfg(test)]
mod tests;

use serde::ser::{SerializeMap, Serializer};
use serde::Serialize;
use serde_json::{Map, Value};

pub use parse::{parse_collection, parse_variable_file};
pub use read::SchemaError;

/// The v2.1.0 schema URL written into every export.
pub const SCHEMA_V21: &str = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

/// A parsed value plus the number of object keys this schema does not know.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed<T> {
    pub value: T,
    pub unknown_fields: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Collection {
    pub info: Info,
    pub item: Vec<Item>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<Auth>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub event: Vec<Event>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub variable: Vec<Variable>,
    #[serde(
        rename = "protocolProfileBehavior",
        skip_serializing_if = "Option::is_none"
    )]
    pub protocol_profile_behavior: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Info {
    #[serde(rename = "_postman_id", skip_serializing_if = "Option::is_none")]
    pub postman_id: Option<String>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Schema URL as found (empty when absent); detection reads it.
    pub schema: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Item {
    Folder(Folder),
    Request(Box<RequestItem>),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Folder {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub item: Vec<Item>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<Auth>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub event: Vec<Event>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub variable: Vec<Variable>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RequestItem {
    pub name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub event: Vec<Event>,
    #[serde(
        rename = "protocolProfileBehavior",
        skip_serializing_if = "Option::is_none"
    )]
    pub protocol_profile_behavior: Option<Map<String, Value>>,
    pub request: Request,
    /// Saved examples are dropped (plan 09 §3); only their number is kept.
    #[serde(skip)]
    pub response_count: u32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Request {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<Auth>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub header: Vec<Header>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Body>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<Url>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Url {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub host: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub query: Vec<QueryParam>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub variable: Vec<Variable>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct QueryParam {
    pub key: Option<String>,
    pub value: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    pub disabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Header {
    pub key: String,
    pub value: String,
    #[serde(skip_serializing_if = "is_false")]
    pub disabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Body {
    /// `raw`, `urlencoded`, `formdata`, `file`, `graphql` (others kept as found).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub urlencoded: Vec<FormParam>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub formdata: Vec<FormParam>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<FileBody>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graphql: Option<GraphqlBody>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<BodyOptions>,
    #[serde(skip_serializing_if = "is_false")]
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct BodyOptions {
    pub raw: RawOptions,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct RawOptions {
    /// `json`, `xml`, `text`, `javascript`, `html`.
    pub language: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct FormParam {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// `src` is a string or an array in Postman; normalised to a list.
    #[serde(serialize_with = "ser_src", skip_serializing_if = "Vec::is_empty")]
    pub src: Vec<String>,
    /// `text` or `file`.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(rename = "contentType", skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    pub disabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct FileBody {
    pub src: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct GraphqlBody {
    pub query: String,
    /// JSON text; an object found on import is re-serialised compactly.
    pub variables: String,
}

/// Auth in both forms reads into this; it writes the v2.1 form `{type, <type>: [..]}`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Auth {
    pub kind: String,
    pub params: Vec<AuthParam>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AuthParam {
    pub key: String,
    pub value: Value,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

impl Auth {
    /// The parameter `key` as text: strings as-is, numbers and booleans as JSON text.
    pub fn get(&self, key: &str) -> Option<String> {
        let param = self.params.iter().find(|p| p.key == key)?;
        match &param.value {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }
}

impl Serialize for Auth {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let with_params = self.kind != "noauth";
        let mut map = serializer.serialize_map(Some(if with_params { 2 } else { 1 }))?;
        map.serialize_entry("type", &self.kind)?;
        if with_params {
            map.serialize_entry(&self.kind, &self.params)?;
        }
        map.end()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Event {
    /// `prerequest` or `test`.
    pub listen: String,
    pub script: Script,
    #[serde(skip_serializing_if = "is_false")]
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Script {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub exec: Vec<String>,
}

impl Script {
    /// The source text: `exec` lines joined with `\n`.
    pub fn source(&self) -> String {
        self.exec.join("\n")
    }
}

/// Collection, folder and URL path variables.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Variable {
    pub key: String,
    /// Scalars as text (numbers/booleans as JSON text); null or absent → empty.
    pub value: String,
    /// `string`, `boolean`, `any`, `number`, `secret` …
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    pub disabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// An environment or globals file (`*.postman_environment.json`, `*.postman_globals.json`).
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct VariableFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub values: Vec<EnvValue>,
    /// `environment` or `globals` (absent in some environment exports).
    #[serde(
        rename = "_postman_variable_scope",
        skip_serializing_if = "Option::is_none"
    )]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct EnvValue {
    pub key: String,
    pub value: String,
    /// `default` or `secret`.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub enabled: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn ser_src<S: Serializer>(src: &[String], serializer: S) -> Result<S::Ok, S::Error> {
    match src {
        [one] => serializer.serialize_str(one),
        many => many.serialize(serializer),
    }
}
