use serde_json::json;

use super::*;
use crate::error::ErrorCode;

fn scripts(pre: &str, post: &str) -> Scripts {
    Scripts {
        pre_request: pre.into(),
        post_response: post.into(),
    }
}

#[test]
fn missing_or_null_section_is_empty_scripts() {
    assert_eq!(parse_scripts("f", None).unwrap(), Scripts::default());
    assert_eq!(
        parse_scripts("f", Some(&Value::Null)).unwrap(),
        Scripts::default()
    );
}

#[test]
fn parses_both_scripts_and_defaults_missing_one() {
    let value = json!({"pre_request": "a();\n"});

    let parsed = parse_scripts("f", Some(&value)).unwrap();

    assert_eq!(parsed, scripts("a();\n", ""));
}

#[test]
fn rejects_wrong_shapes_naming_file_and_field() {
    let cases = [
        (json!("x"), "`scripts`"),
        (json!({"pre_request": ["x"]}), "`scripts.pre_request`"),
        (
            json!({"post_response": {"a": 1}}),
            "`scripts.post_response`",
        ),
    ];
    for (value, field) in cases {
        let error = parse_scripts("a.http.yaml", Some(&value)).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput);
        assert!(
            error.message.starts_with("a.http.yaml: "),
            "{}",
            error.message
        );
        assert!(error.message.contains(field), "{}", error.message);
    }
}

#[test]
fn rejects_a_script_over_256_kib() {
    let at_limit = "x".repeat(MAX_SCRIPT_BYTES);
    let over = "x".repeat(MAX_SCRIPT_BYTES + 1);

    assert!(validate_scripts("f", &scripts(&at_limit, "")).is_ok());
    let error = validate_scripts("f", &scripts("", &over)).unwrap_err();

    assert!(
        error.message.contains("`scripts.post_response`") && error.message.contains("256 KiB"),
        "{}",
        error.message
    );
}

#[test]
fn allows_newlines_and_tabs() {
    assert!(validate_scripts("f", &scripts("a\n\tb\r\n", "")).is_ok());
}

#[test]
fn emit_omits_empty_scripts_and_keeps_unknown_keys() {
    let raw = json!({"pre_request": "old", "future": 1});

    assert_eq!(emit_scripts(&Scripts::default(), None), None);
    assert_eq!(
        emit_scripts(&scripts("", "b"), None),
        Some(json!({"post_response": "b"}))
    );
    assert_eq!(
        emit_scripts(&Scripts::default(), Some(&raw)),
        Some(json!({"future": 1}))
    );
    assert_eq!(
        emit_scripts(&scripts("new", ""), Some(&raw)),
        Some(json!({"pre_request": "new", "future": 1}))
    );
}

#[test]
fn with_scripts_replaces_or_removes_the_section() {
    let extra = Extra::from([
        ("other".to_string(), json!(true)),
        (SCRIPTS.to_string(), json!({"pre_request": "a"})),
    ]);

    let cleared = with_scripts(&extra, &Scripts::default());
    let set = with_scripts(&extra, &scripts("", "b"));

    assert_eq!(cleared, Extra::from([("other".to_string(), json!(true))]));
    assert_eq!(set[SCRIPTS], json!({"post_response": "b"}));
    assert_eq!(set["other"], json!(true));
}
