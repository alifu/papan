//! YAML in and out. Parsing is bounded (size, alias budget, conflict markers) and maps
//! failures to `invalid_input` naming the file; emitting goes through a tree whose
//! mappings are `BTreeMap`s so keys are sorted at every level (plan §3.1).

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::{Serialize, Serializer};

use crate::error::AppError;

/// Generous for hand-written request files, small enough that an alias bomb fails fast.
const MAX_NODES: usize = 20_000;
const MAX_ALIASES: usize = 1_000;
const MAX_DEPTH: usize = 64;
/// Parser text quoted in a user-facing message, and in the log detail.
const MAX_MESSAGE_CHARS: usize = 200;
const MAX_DETAIL_CHARS: usize = 1_000;

/// A YAML value with sorted mappings.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Seq(Vec<Node>),
    Map(BTreeMap<String, Node>),
}

impl From<&serde_json::Value> for Node {
    fn from(value: &serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => Node::Null,
            serde_json::Value::Bool(flag) => Node::Bool(*flag),
            serde_json::Value::Number(number) => Node::Number(number.clone()),
            serde_json::Value::String(text) => Node::String(text.clone()),
            serde_json::Value::Array(items) => Node::Seq(items.iter().map(Node::from).collect()),
            serde_json::Value::Object(map) => Node::Map(
                map.iter()
                    .map(|(key, value)| (key.clone(), Node::from(value)))
                    .collect(),
            ),
        }
    }
}

impl Serialize for Node {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Node::Null => serializer.serialize_unit(),
            Node::Bool(flag) => serializer.serialize_bool(*flag),
            Node::Number(number) => number.serialize(serializer),
            Node::String(text) => serializer.serialize_str(text),
            Node::Seq(items) => items.serialize(serializer),
            Node::Map(map) => map.serialize(serializer),
        }
    }
}

/// Emits a top-level mapping in canonical form. Callers omit default-valued fields.
pub fn emit(map: &BTreeMap<String, Node>) -> Result<String, AppError> {
    let mut options = serde_saphyr::SerializerOptions::default();
    options.compact_list_indent = false;
    // Long single-line strings (URLs) stay on one line; multi-line strings still use `|`.
    options.folded_wrap_chars = usize::MAX;
    serde_saphyr::to_string_with_options(map, options).map_err(|source| {
        AppError::internal("Could not write the file.").with_detail(source.to_string())
    })
}

/// Parses `bytes` from the file called `label` into `T`.
pub fn parse<T: DeserializeOwned>(label: &str, bytes: &[u8]) -> Result<T, AppError> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| invalid(label, "the file is not valid UTF-8."))?;
    if has_conflict_markers(text) {
        return Err(invalid(
            label,
            "the file contains Git conflict markers; resolve the conflict first.",
        ));
    }
    serde_saphyr::from_str_with_options(text, options()).map_err(|error| {
        let summary = first_line(&error.without_snippet().to_string());
        invalid(label, &describe(&truncate(&summary, MAX_MESSAGE_CHARS)))
            .with_detail(truncate(&summary, MAX_DETAIL_CHARS))
    })
}

/// `invalid_input` whose message names the file.
pub fn invalid(label: &str, problem: &str) -> AppError {
    AppError::invalid_input(format!("{label}: {problem}"))
}

fn options() -> serde_saphyr::Options {
    let mut budget = serde_saphyr::Budget::default();
    budget.max_nodes = MAX_NODES;
    budget.max_aliases = MAX_ALIASES;
    budget.max_depth = MAX_DEPTH;
    let mut options = serde_saphyr::Options::default();
    options.budget = Some(budget);
    options
}

fn has_conflict_markers(text: &str) -> bool {
    text.lines().any(|line| {
        line.starts_with("<<<<<<< ") || line == "=======" || line.starts_with(">>>>>>> ")
    })
}

/// The first line of the parser's error, without the `error: ` prefix or a snippet.
fn first_line(rendered: &str) -> String {
    rendered
        .lines()
        .next()
        .unwrap_or_default()
        .trim_start_matches("error: ")
        .trim()
        .to_string()
}

fn truncate(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_string(),
    }
}

/// One short sentence from the parser's first line.
fn describe(first: &str) -> String {
    let lower = first.to_lowercase();
    if lower.contains("budget") || lower.contains("alias") {
        "too many aliases or values (alias expansion limit reached).".to_string()
    } else if lower.contains("expected mapping") {
        format!("the file must be a mapping of fields ({first}).")
    } else if is_field_problem(&lower) {
        format!("{first}.")
    } else {
        format!("YAML syntax error ({first}).")
    }
}

fn is_field_problem(lower: &str) -> bool {
    [
        "missing field",
        "duplicate mapping key",
        "unknown field",
        "invalid type",
        "invalid value",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct Probe {
        #[allow(dead_code)]
        name: String,
    }

    #[test]
    fn emit_sorts_keys_and_indents_sequences() {
        let map = BTreeMap::from([
            ("zeta".to_string(), Node::String("z".into())),
            (
                "alpha".to_string(),
                Node::Seq(vec![Node::String("a".into()), Node::Bool(true)]),
            ),
            ("mid".to_string(), Node::Null),
        ]);

        let text = emit(&map).unwrap();

        assert_eq!(text, "alpha:\n  - a\n  - true\nmid: null\nzeta: z\n");
    }

    #[test]
    fn node_from_json_keeps_structure() {
        let value = serde_json::json!({"b": [1, null], "a": {"c": false}});

        let node = Node::from(&value);

        let Node::Map(map) = node else {
            panic!("not a map")
        };
        assert_eq!(map.keys().collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn parse_names_file_and_reason() {
        let cases = [
            ("name: a\nname: b\n", "duplicate"),
            ("- a\n", "mapping"),
            ("name: [a\n", "syntax"),
            (
                "x: 1\n<<<<<<< HEAD\nname: a\n=======\nname: b\n>>>>>>> x\n",
                "conflict",
            ),
            ("other: 1\n", "name"),
        ];
        for (text, word) in cases {
            let error = parse::<Probe>("f.http.yaml", text.as_bytes()).unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidInput);
            assert!(
                error.message.starts_with("f.http.yaml: "),
                "{}",
                error.message
            );
            assert!(
                error.message.contains(word),
                "{} lacks {word}",
                error.message
            );
        }
    }

    #[test]
    fn parse_errors_are_short_and_carry_no_source_snippet() {
        let long_key = "k".repeat(5_000);
        let text = format!("{long_key}: a\n{long_key}: b\n");

        let error = parse::<Probe>("f.http.yaml", text.as_bytes()).unwrap_err();

        let detail = error.detail.unwrap();
        assert!(
            error.message.chars().count() < 300,
            "{}",
            error.message.len()
        );
        assert!(detail.chars().count() <= MAX_DETAIL_CHARS + 1);
        assert!(
            !detail.contains('\n') && !detail.contains(" | "),
            "{detail}"
        );
    }

    #[test]
    fn parse_rejects_invalid_utf8() {
        let error = parse::<Probe>("f.http.yaml", &[0xff, 0xfe]).unwrap_err();

        assert!(error.message.contains("UTF-8"));
    }
}
