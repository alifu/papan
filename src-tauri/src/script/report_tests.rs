use super::*;

fn change(name: &str, value: Option<&str>, secret: bool) -> VarChange {
    VarChange {
        scope: Scope::Environment,
        name: name.into(),
        value: value.map(str::to_string),
        secret,
    }
}

#[test]
fn text_masks_every_secret_longest_first() {
    let redaction = Redaction::new(vec!["abc".into(), "abcdef".into(), String::new()], None);
    assert_eq!(
        redaction.text("x abcdef y abc z"),
        "x [REDACTED] y [REDACTED] z"
    );
    assert_eq!(redaction.text("nothing"), "nothing");
}

#[test]
fn the_callers_redactor_runs_after_the_secret_list() {
    let extra: Redactor = Arc::new(|text: &str| text.replace("p%40ss", "[REDACTED]"));
    let redaction = Redaction::new(vec!["p@ss".into()], Some(&extra));
    assert_eq!(redaction.text("p@ss p%40ss"), "[REDACTED] [REDACTED]");
}

#[test]
fn a_secret_change_is_listed_by_name_only() {
    let redaction = Redaction::new(vec!["hunter2".into()], None);
    let secret = redaction.change(&change("token", Some("hunter2"), true));
    assert_eq!(
        (secret.value, secret.secret, secret.removed),
        (None, true, false)
    );

    let plain = redaction.change(&change("copy", Some("is hunter2"), false));
    assert_eq!(plain.value.as_deref(), Some("is [REDACTED]"));

    let removed = redaction.change(&change("gone", None, false));
    assert_eq!((removed.value, removed.removed), (None, true));
}

#[test]
fn console_tests_and_errors_are_masked() {
    let redaction = Redaction::new(vec!["hunter2".into()], None);
    let line = redaction.console(&ConsoleLine {
        level: ConsoleLevel::Log,
        text: "pw=hunter2".into(),
    });
    assert_eq!(line.text, "pw=[REDACTED]");
    let test = redaction.test(&TestResult {
        name: "hunter2 works".into(),
        passed: false,
        error: Some("expected \"hunter2\" to equal 1".into()),
    });
    assert_eq!(test.name, "[REDACTED] works");
    assert_eq!(
        test.error.as_deref(),
        Some("expected \"[REDACTED]\" to equal 1")
    );
    let error = redaction.error(&ScriptError {
        kind: ScriptErrorKind::Exception,
        message: "Error: hunter2".into(),
        script: Some("request".into()),
        line: Some(3),
    });
    assert_eq!(error.message, "Error: [REDACTED]");
    assert_eq!(error.line, Some(3));
}

#[test]
fn the_report_serialises_in_snake_case() {
    let report = ScriptReport::failed(ScriptPhase::PreRequest, ScriptErrorKind::Timeout, "slow");
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["phase"], "pre_request");
    assert_eq!(json["error"]["kind"], "timeout");
    assert_eq!(json["console_truncated"], false);
}

#[test]
fn a_json_escaped_secret_is_masked() {
    let redaction = Redaction::new(vec!["pa\"ss\\w\n".into()], None);
    assert_eq!(
        redaction.text(r#"{"p":"pa\"ss\\w\n"}"#),
        r#"{"p":"[REDACTED]"}"#
    );
    assert_eq!(redaction.text("raw pa\"ss\\w\n"), "raw [REDACTED]");
}

#[test]
fn text_is_masked_before_it_is_cut() {
    let redaction = Redaction::new(vec!["topsecret".into()], None);
    let clipped = redaction.clipped(&format!("{}topsecret", "x".repeat(95)), 100);
    assert!(!clipped.contains("tops"), "{clipped}");
    assert!(clipped.len() <= 100 + '…'.len_utf8());
}

#[test]
fn a_text_the_host_cut_loses_the_tail_that_may_start_a_secret() {
    let secret = "s".repeat(60);
    let redaction = Redaction::new(vec![secret.clone()], None);
    // Masking shrinks the text below the limit, so the cut secret would survive it.
    let raw = format!("{}{}…", secret.repeat(5), &secret[..40]);
    let clipped = redaction.clipped(&raw, 4096);
    assert!(!clipped.contains("ssss"), "{clipped}");
    assert!(clipped.ends_with('…'));
}
