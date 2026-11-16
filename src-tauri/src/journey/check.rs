//! What a Condition or Assert node tests, and where a Request node's outputs come from
//! (plan 13 §4): values read from the last response or the journey variables.

use std::collections::BTreeMap;

use serde_json::Value;

use super::model::{Check, CheckOp, CheckSource, Output};
use crate::mock::request::json_path;

/// The answer a Request node got, as the runner keeps it: the body is text, capped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sent {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// What a check reads from.
pub struct Scope<'a> {
    pub vars: &'a BTreeMap<String, String>,
    pub last: Option<&'a Sent>,
}

/// `{{name}}` replaced by the journey variable; an unknown name stays as typed.
pub fn fill(text: &str, vars: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                let name = after[..end].trim();
                match vars.get(name) {
                    Some(value) => out.push_str(value),
                    None => out.push_str(&rest[start..start + 2 + end + 2]),
                }
                rest = &after[end + 2..];
            }
            None => {
                out.push_str(&rest[start..]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

fn text_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// The value of `source`/`name`; `None` when it is not there.
pub fn read(source: CheckSource, name: &str, scope: &Scope<'_>) -> Option<String> {
    match source {
        CheckSource::Status => scope.last.map(|sent| sent.status.to_string()),
        CheckSource::Header => {
            scope.last?.headers.iter().find_map(|(key, value)| {
                key.eq_ignore_ascii_case(name.trim()).then(|| value.clone())
            })
        }
        CheckSource::Body => {
            let parsed: Value = serde_json::from_str(&scope.last?.body).ok()?;
            json_path(&parsed, name).map(text_of)
        }
        CheckSource::Variable => scope.vars.get(name.trim()).cloned(),
    }
}

/// The value a Request node keeps for `output`.
pub fn extract(output: &Output, scope: &Scope<'_>) -> Option<String> {
    read(output.source, &output.path, scope)
}

fn numbers(a: &str, b: &str) -> Option<(f64, f64)> {
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

/// Whether the check holds. `Err` says why it cannot be decided (no response yet, text
/// that is not a number for a comparison).
pub fn evaluate(check: &Check, scope: &Scope<'_>) -> Result<bool, String> {
    let name = fill(&check.name, scope.vars);
    let found = read(check.source, &name, scope);
    if matches!(
        check.source,
        CheckSource::Status | CheckSource::Header | CheckSource::Body
    ) && scope.last.is_none()
    {
        return Err("There is no response to check yet.".into());
    }
    let expected = fill(&check.value, scope.vars);
    let Some(actual) = found else {
        return Ok(matches!(check.op, CheckOp::NotExists | CheckOp::Ne));
    };
    Ok(match check.op {
        CheckOp::Exists => true,
        CheckOp::NotExists => false,
        CheckOp::Eq => match numbers(&actual, &expected) {
            Some((a, b)) => (a - b).abs() < f64::EPSILON,
            None => actual == expected,
        },
        CheckOp::Ne => match numbers(&actual, &expected) {
            Some((a, b)) => (a - b).abs() >= f64::EPSILON,
            None => actual != expected,
        },
        CheckOp::Contains => actual.contains(&expected),
        CheckOp::Gt | CheckOp::Ge | CheckOp::Lt | CheckOp::Le => {
            let (a, b) = numbers(&actual, &expected)
                .ok_or_else(|| format!("\"{actual}\" and \"{expected}\" are not both numbers."))?;
            match check.op {
                CheckOp::Gt => a > b,
                CheckOp::Ge => a >= b,
                CheckOp::Lt => a < b,
                _ => a <= b,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sent() -> Sent {
        Sent {
            status: 201,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: r#"{"id": 7, "user": {"name": "Ann", "tags": ["a", "b"]}}"#.into(),
        }
    }

    fn check(source: CheckSource, name: &str, op: CheckOp, value: &str) -> Check {
        Check {
            source,
            name: name.into(),
            op,
            value: value.into(),
        }
    }

    #[test]
    fn fills_known_variables_and_leaves_unknown_ones() {
        let vars = BTreeMap::from([("id".to_string(), "7".to_string())]);
        assert_eq!(
            fill("/u/{{id}}/{{ id }}/{{x}}/{{open", &vars),
            "/u/7/7/{{x}}/{{open"
        );
    }

    #[test]
    fn reads_status_headers_body_and_variables() {
        let vars = BTreeMap::from([("t".to_string(), "abc".to_string())]);
        let last = sent();
        let scope = Scope {
            vars: &vars,
            last: Some(&last),
        };
        assert_eq!(
            read(CheckSource::Status, "", &scope).as_deref(),
            Some("201")
        );
        assert_eq!(
            read(CheckSource::Header, "content-type", &scope).as_deref(),
            Some("application/json")
        );
        assert_eq!(
            read(CheckSource::Body, "user.tags[1]", &scope).as_deref(),
            Some("b")
        );
        assert_eq!(read(CheckSource::Body, "id", &scope).as_deref(), Some("7"));
        assert_eq!(read(CheckSource::Body, "nope", &scope), None);
        assert_eq!(
            read(CheckSource::Variable, "t", &scope).as_deref(),
            Some("abc")
        );
    }

    #[test]
    fn evaluates_operators() {
        let vars = BTreeMap::from([("want".to_string(), "201".to_string())]);
        let last = sent();
        let scope = Scope {
            vars: &vars,
            last: Some(&last),
        };
        let ok = |c: Check| evaluate(&c, &scope).unwrap();
        assert!(ok(check(CheckSource::Status, "", CheckOp::Eq, "{{want}}")));
        assert!(ok(check(CheckSource::Status, "", CheckOp::Ge, "200")));
        assert!(!ok(check(CheckSource::Status, "", CheckOp::Lt, "200")));
        assert!(ok(check(
            CheckSource::Body,
            "user.name",
            CheckOp::Contains,
            "An"
        )));
        assert!(ok(check(
            CheckSource::Body,
            "missing",
            CheckOp::NotExists,
            ""
        )));
        assert!(ok(check(CheckSource::Body, "id", CheckOp::Exists, "")));
        assert!(ok(check(CheckSource::Body, "missing", CheckOp::Ne, "x")));
        assert!(evaluate(
            &check(CheckSource::Body, "user.name", CheckOp::Gt, "1"),
            &scope
        )
        .is_err());
    }

    #[test]
    fn a_response_check_without_a_response_cannot_be_decided() {
        let vars = BTreeMap::new();
        let scope = Scope {
            vars: &vars,
            last: None,
        };
        assert!(evaluate(&check(CheckSource::Status, "", CheckOp::Eq, "200"), &scope).is_err());
        assert!(!evaluate(&check(CheckSource::Variable, "x", CheckOp::Eq, "1"), &scope).unwrap());
    }
}
