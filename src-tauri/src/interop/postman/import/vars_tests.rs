use super::*;
use crate::interop::postman::types::ReportLevel;

fn raw<'a>(key: &'a str, value: &'a str, secret: bool) -> RawVar<'a> {
    RawVar {
        key,
        value,
        enabled: true,
        secret,
    }
}

#[test]
fn secret_values_never_enter_the_table() {
    let mut report = Report::new();
    let vars = [raw("base", "https://x", false), raw("pw", "hunter2", true)];

    let planned = plan_vars("e.json", "values", &vars, &mut report);

    assert_eq!(planned.table["base"].value, "https://x");
    assert!(planned.table["pw"].secret);
    assert_eq!(planned.table["pw"].value, "");
    assert_eq!(planned.secrets.get("pw"), Some("hunter2"));
    assert!(planned.candidates.is_empty());
    assert!(report.items().is_empty());
}

#[test]
fn invalid_duplicate_and_oversized_variables_are_skipped_with_their_path() {
    let mut report = Report::new();
    let big = "x".repeat(MAX_VALUE_BYTES + 1);
    let vars = [
        raw("$bad", "1", false),
        raw("a{b}", "1", false),
        raw("ok", "1", false),
        raw("ok", "2", false),
        raw("big", &big, false),
    ];

    let planned = plan_vars("e.json", "values", &vars, &mut report);

    assert_eq!(planned.table.len(), 1);
    let paths: Vec<&str> = report
        .items()
        .iter()
        .map(|item| item.path.as_str())
        .collect();
    assert_eq!(paths, ["values[0]", "values[1]", "values[3]", "values[4]"]);
    assert!(report
        .items()
        .iter()
        .all(|item| item.level == ReportLevel::Skipped));
    assert!(!report.items()[0].message.starts_with("e.json"));
}

#[test]
fn more_than_500_variables_are_cut() {
    let mut report = Report::new();
    let names: Vec<String> = (0..MAX_VARIABLES + 3).map(|n| format!("v{n}")).collect();
    let vars: Vec<RawVar<'_>> = names.iter().map(|name| raw(name, "", false)).collect();

    let planned = plan_vars("c.json", "variable", &vars, &mut report);

    assert_eq!(planned.table.len(), MAX_VARIABLES);
    assert_eq!(report.items().len(), 1);
}

#[test]
fn sensitive_names_are_candidates_and_the_choice_moves_values() {
    let mut report = Report::new();
    let vars = [raw("apiToken", "abc", false), raw("host", "h", false)];
    let planned = plan_vars("e.json", "values", &vars, &mut report);

    let chosen = apply_choice(
        &planned,
        &["apiToken".into(), "host".into(), "missing".into()],
    );

    assert_eq!(planned.candidates, ["apiToken"]);
    assert!(chosen.table["apiToken"].secret);
    assert_eq!(chosen.table["apiToken"].value, "");
    assert_eq!(chosen.secrets.get("apiToken"), Some("abc"));
    assert!(chosen.table["host"].secret);
    assert_eq!(chosen.secrets.len(), 2);
}

#[test]
fn unknown_dynamics_are_reported_once() {
    let text = "{{$randomEmail}} {{$uuid}} {{$randomEmail}} {{$timestamp}} {{$a b}} {{$x";

    assert_eq!(unknown_dynamics(text), ["$randomEmail"]);
}

#[test]
fn postman_forms_read_flags() {
    let collection = [schema::Variable {
        key: "a".into(),
        value: "1".into(),
        kind: Some("secret".into()),
        disabled: true,
        description: None,
    }];
    let values = [schema::EnvValue {
        key: "b".into(),
        value: "2".into(),
        kind: Some("default".into()),
        enabled: false,
    }];

    let a = &from_collection(&collection)[0];
    let b = &from_values(&values)[0];

    assert!(a.secret && !a.enabled);
    assert!(!b.secret && !b.enabled);
}
