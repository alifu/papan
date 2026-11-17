//! Variable tables in YAML (plan 04 §3.1): `environments/<stem>.env.yaml`, the
//! `variables` key of `papan.collection.yaml`, and `<app data>/globals.yaml`.
//! Unknown keys are kept, per file and per variable; defaults are omitted on write.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value as Json;

use super::model::check_name;
use super::yaml::{self, Node};
use crate::error::AppError;

pub const VARIABLES_KEY: &str = "variables";
pub const MAX_VARIABLES: usize = 500;
pub const MAX_VAR_NAME_CHARS: usize = 128;
pub const MAX_VALUE_BYTES: usize = 64 * 1024;

type Extra = BTreeMap<String, Json>;

/// One variable as stored. A secret never carries `value`.
#[derive(Debug, Clone, PartialEq)]
pub struct VarEntry {
    pub value: String,
    pub enabled: bool,
    pub secret: bool,
    pub extra: serde_json::Map<String, Json>,
}

impl VarEntry {
    pub fn new(value: String, enabled: bool, secret: bool) -> Self {
        Self {
            value: if secret { String::new() } else { value },
            enabled,
            secret,
            extra: serde_json::Map::new(),
        }
    }

    fn to_node(&self) -> Node {
        let mut map: BTreeMap<String, Node> = self
            .extra
            .iter()
            .map(|(key, value)| (key.clone(), Node::from(value)))
            .collect();
        if !self.enabled {
            map.insert("enabled".into(), Node::Bool(false));
        }
        if self.secret {
            map.insert("secret".into(), Node::Bool(true));
        } else if !self.value.is_empty() {
            map.insert("value".into(), Node::String(self.value.clone()));
        }
        Node::Map(map)
    }
}

/// Variables keyed (and so sorted) by name.
pub type VarTable = BTreeMap<String, VarEntry>;

fn field_error(label: &str, field: &str, problem: &str) -> AppError {
    yaml::invalid(label, &format!("field `{field}` {problem}"))
}

/// Checks a variable name; `label` names the file or the input.
pub fn check_var_name(label: &str, name: &str) -> Result<(), AppError> {
    let field = format!("{VARIABLES_KEY}.{name}");
    let length = name.chars().count();
    if name.trim().is_empty() {
        return Err(field_error(
            label,
            VARIABLES_KEY,
            "has a variable with an empty name.",
        ));
    }
    let problem = if length > MAX_VAR_NAME_CHARS {
        Some("has a name longer than 128 characters.")
    } else if name.trim() != name {
        Some("has a name with leading or trailing spaces.")
    } else if name.chars().any(char::is_control) {
        Some("has a name with control characters.")
    } else if name.contains(['{', '}']) {
        Some("has a name containing `{` or `}`.")
    } else if name.starts_with('$') {
        Some("has a name starting with `$` (reserved for dynamic variables).")
    } else {
        None
    };
    problem.map_or(Ok(()), |problem| Err(field_error(label, &field, problem)))
}

/// Checks an initial value.
pub fn check_value(label: &str, name: &str, value: &str) -> Result<(), AppError> {
    if value.len() > MAX_VALUE_BYTES {
        let field = format!("{VARIABLES_KEY}.{name}.value");
        return Err(field_error(label, &field, "is longer than 64 KiB."));
    }
    Ok(())
}

fn flag(label: &str, field: &str, value: Option<&Json>, default: bool) -> Result<bool, AppError> {
    match value {
        None | Some(Json::Null) => Ok(default),
        Some(Json::Bool(flag)) => Ok(*flag),
        Some(_) => Err(field_error(label, field, "must be true or false.")),
    }
}

fn parse_entry(label: &str, name: &str, raw: &Json) -> Result<VarEntry, AppError> {
    let field = format!("{VARIABLES_KEY}.{name}");
    let Json::Object(map) = raw else {
        return Err(field_error(label, &field, "must be a mapping."));
    };
    let enabled = flag(label, &format!("{field}.enabled"), map.get("enabled"), true)?;
    let secret = flag(label, &format!("{field}.secret"), map.get("secret"), false)?;
    let value = match map.get("value") {
        None | Some(Json::Null) => String::new(),
        Some(Json::String(text)) => text.clone(),
        Some(_) => {
            return Err(field_error(
                label,
                &format!("{field}.value"),
                "must be a string.",
            ))
        }
    };
    if secret && !value.is_empty() {
        tracing::warn!(file = label, "ignoring a value stored on a secret variable");
    }
    check_value(label, name, &value)?;
    let extra = map
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "enabled" | "secret" | "value"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    Ok(VarEntry {
        extra,
        ..VarEntry::new(value, enabled, secret)
    })
}

/// Parses and validates the `variables` value (absent or null = empty).
pub fn parse_variables(label: &str, raw: Option<&Json>) -> Result<VarTable, AppError> {
    let map = match raw {
        None | Some(Json::Null) => return Ok(VarTable::new()),
        Some(Json::Object(map)) => map,
        Some(_) => {
            return Err(field_error(
                label,
                VARIABLES_KEY,
                "must be a mapping of names.",
            ))
        }
    };
    if map.len() > MAX_VARIABLES {
        return Err(field_error(
            label,
            VARIABLES_KEY,
            "has more than 500 variables.",
        ));
    }
    map.iter()
        .map(|(name, raw)| {
            check_var_name(label, name)?;
            Ok((name.clone(), parse_entry(label, name, raw)?))
        })
        .collect()
}

/// The `variables` node to write, or `None` when the table is empty (omitted).
pub fn variables_node(table: &VarTable) -> Option<Node> {
    (!table.is_empty()).then(|| {
        Node::Map(
            table
                .iter()
                .map(|(name, entry)| (name.clone(), entry.to_node()))
                .collect(),
        )
    })
}

fn emit(extra: &Extra, table: &VarTable, known: Vec<(&str, Node)>) -> Result<String, AppError> {
    let map = extra
        .iter()
        .map(|(key, value)| (key.clone(), Node::from(value)))
        .chain(known.into_iter().map(|(key, node)| (key.to_string(), node)))
        .chain(variables_node(table).map(|node| (VARIABLES_KEY.to_string(), node)))
        .collect();
    yaml::emit(&map)
}

#[derive(Debug, Deserialize)]
struct RawEnv {
    name: String,
    #[serde(default)]
    variables: Option<Json>,
    #[serde(flatten)]
    extra: Extra,
}

#[derive(Debug, Deserialize)]
struct RawGlobals {
    #[serde(default)]
    variables: Option<Json>,
    #[serde(flatten)]
    extra: Extra,
}

/// `environments/<stem>.env.yaml`.
#[derive(Debug, Clone, PartialEq)]
pub struct EnvFile {
    pub name: String,
    pub variables: VarTable,
    pub extra: Extra,
}

impl EnvFile {
    pub fn new(name: String) -> Self {
        Self {
            name,
            variables: VarTable::new(),
            extra: Extra::new(),
        }
    }

    pub fn parse(label: &str, bytes: &[u8]) -> Result<Self, AppError> {
        let raw: RawEnv = yaml::parse(label, bytes)?;
        check_name(label, &raw.name)?;
        Ok(Self {
            variables: parse_variables(label, raw.variables.as_ref())?,
            name: raw.name,
            extra: raw.extra,
        })
    }

    pub fn to_yaml(&self) -> Result<String, AppError> {
        let known = vec![("name", Node::String(self.name.clone()))];
        emit(&self.extra, &self.variables, known)
    }
}

/// `<app data>/globals.yaml`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GlobalsFile {
    pub variables: VarTable,
    pub extra: Extra,
}

impl GlobalsFile {
    pub fn parse(label: &str, bytes: &[u8]) -> Result<Self, AppError> {
        let raw: RawGlobals = yaml::parse(label, bytes)?;
        Ok(Self {
            variables: parse_variables(label, raw.variables.as_ref())?,
            extra: raw.extra,
        })
    }

    pub fn to_yaml(&self) -> Result<String, AppError> {
        emit(&self.extra, &self.variables, Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_omit_defaults_and_keep_unknown_keys() {
        let text = "name: E\nvariables:\n  a:\n    enabled: true\n    value: x\n    x-k: 1\n  s:\n    secret: true\n    value: leaked\n";

        let file = EnvFile::parse("e.env.yaml", text.as_bytes()).unwrap();

        assert_eq!(file.variables["s"].value, "");
        assert_eq!(
            file.to_yaml().unwrap(),
            "name: E\nvariables:\n  a:\n    value: x\n    x-k: 1\n  s:\n    secret: true\n"
        );
    }

    #[test]
    fn names_and_values_are_checked() {
        assert!(check_var_name("L", "api key").is_ok());
        for bad in ["", " a", "a\u{7}", "a}", "$x", &"n".repeat(129)] {
            let error = check_var_name("L", bad).unwrap_err();
            assert!(error.message.starts_with("L: field `variables"), "{bad:?}");
        }
        assert!(check_value("L", "a", &"x".repeat(MAX_VALUE_BYTES + 1)).is_err());
        let error = EnvFile::parse("e", b"name: E\nvariables:\n  a:\n    value: 1\n").unwrap_err();
        assert!(
            error.message.contains("variables.a.value"),
            "{}",
            error.message
        );
    }

    #[test]
    fn globals_round_trip() {
        let text = "variables:\n  a:\n    value: x\n";
        let file = GlobalsFile::parse("globals.yaml", text.as_bytes()).unwrap();
        assert_eq!(file.to_yaml().unwrap(), text);
        assert_eq!(GlobalsFile::default().to_yaml().unwrap(), "{}\n");
    }
}
