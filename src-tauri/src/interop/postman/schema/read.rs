//! Path-aware readers over `serde_json::Value`: null counts as absent, wrong types name the path.

use std::fmt;

use serde_json::{Map, Value};

/// A wrong type at `path` (dotted/indexed, e.g. `values[3].key`; empty = the document root).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaError {
    pub path: String,
    /// What was expected, e.g. "a string", "an array".
    pub expected: &'static str,
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "the file must be {}.", self.expected)
        } else {
            write!(f, "field `{}` must be {}.", self.path, self.expected)
        }
    }
}

impl std::error::Error for SchemaError {}

pub(super) type Res<T> = Result<T, SchemaError>;

pub(super) fn fail<T>(path: &str, expected: &'static str) -> Res<T> {
    Err(SchemaError {
        path: path.to_string(),
        expected,
    })
}

pub(super) fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

pub(super) fn index(path: &str, i: usize) -> String {
    format!("{path}[{i}]")
}

/// Counts object keys outside each struct's known list.
#[derive(Debug, Default)]
pub(super) struct Reader {
    pub unknown: u32,
}

impl Reader {
    pub fn object<'a>(&mut self, v: &'a Value, path: &str, known: &[&str]) -> Res<Obj<'a>> {
        let Value::Object(map) = v else {
            return fail(path, "an object");
        };
        let extra = map.keys().filter(|k| !known.contains(&k.as_str())).count();
        self.unknown = self
            .unknown
            .saturating_add(u32::try_from(extra).unwrap_or(u32::MAX));
        Ok(Obj {
            map,
            path: path.to_string(),
        })
    }
}

pub(super) struct Obj<'a> {
    pub map: &'a Map<String, Value>,
    pub path: String,
}

impl<'a> Obj<'a> {
    pub fn path(&self, key: &str) -> String {
        join(&self.path, key)
    }

    /// The value at `key`; JSON `null` reads as absent.
    pub fn get(&self, key: &str) -> Option<&'a Value> {
        self.map.get(key).filter(|v| !v.is_null())
    }

    pub fn opt_str(&self, key: &str) -> Res<Option<String>> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(_) => fail(&self.path(key), "a string"),
        }
    }

    pub fn str_or_empty(&self, key: &str) -> Res<String> {
        Ok(self.opt_str(key)?.unwrap_or_default())
    }

    /// Strings as-is; numbers and booleans as JSON text (Postman writes `"port": 8080` too).
    pub fn opt_scalar(&self, key: &str) -> Res<Option<String>> {
        match self.get(key) {
            None => Ok(None),
            Some(v) => scalar(v, &self.path(key)).map(Some),
        }
    }

    pub fn flag(&self, key: &str, default: bool) -> Res<bool> {
        match self.get(key) {
            None => Ok(default),
            Some(Value::Bool(b)) => Ok(*b),
            Some(_) => fail(&self.path(key), "a boolean"),
        }
    }

    /// An array (absent → empty).
    pub fn array(&self, key: &str) -> Res<&'a [Value]> {
        match self.get(key) {
            None => Ok(&[]),
            Some(Value::Array(a)) => Ok(a),
            Some(_) => fail(&self.path(key), "an array"),
        }
    }

    pub fn opt_object(&self, key: &str) -> Res<Option<&'a Map<String, Value>>> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::Object(m)) => Ok(Some(m)),
            Some(_) => fail(&self.path(key), "an object"),
        }
    }

    /// `description`: a string or `{content, type, version}`.
    pub fn description(&self, key: &str) -> Res<Option<String>> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(Value::Object(m)) => match m.get("content") {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(s)) => Ok(Some(s.clone())),
                Some(_) => fail(&join(&self.path(key), "content"), "a string"),
            },
            Some(_) => fail(&self.path(key), "a string or an object"),
        }
    }

    /// A string or an array of strings (e.g. `exec`, `src`), normalised to a list.
    pub fn str_list(&self, key: &str) -> Res<Vec<String>> {
        let path = self.path(key);
        match self.get(key) {
            None => Ok(Vec::new()),
            Some(Value::String(s)) => Ok(vec![s.clone()]),
            Some(Value::Array(items)) => items
                .iter()
                .enumerate()
                .filter(|(_, v)| !v.is_null())
                .map(|(i, v)| match v {
                    Value::String(s) => Ok(s.clone()),
                    _ => fail(&index(&path, i), "a string"),
                })
                .collect(),
            Some(_) => fail(&path, "a string or an array"),
        }
    }
}

pub(super) fn scalar(v: &Value, path: &str) -> Res<String> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Null => Ok(String::new()),
        _ => fail(path, "a string, number or boolean"),
    }
}
