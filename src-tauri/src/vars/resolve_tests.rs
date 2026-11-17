use std::collections::BTreeMap;

use serde::Deserialize;

use super::*;
use crate::vars::dynamic::{FixedDynamics, SystemDynamics};
use crate::vars::scopes::{Layer, Value};
use crate::vars::types::VarSource;

#[derive(Debug, Deserialize)]
struct Case {
    case: String,
    layers: Vec<Layer>,
    text: String,
    expected: String,
    unresolved: Vec<String>,
}

fn global(vars: &[(&str, Value)]) -> Scopes {
    Scopes::new(vec![Layer {
        source: VarSource::Global,
        vars: vars
            .iter()
            .map(|(name, value)| (name.to_string(), value.clone()))
            .collect(),
    }])
}

#[test]
fn shared_resolve_cases_pass() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/vars/resolve-cases.json"
    );
    let cases: Vec<Case> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert!(cases.len() >= 15, "fixtures missing");

    for case in cases {
        let mut report = Report::default();
        let scopes = Scopes::new(case.layers);

        let resolved = resolve_text(&case.text, &scopes, &SystemDynamics, &mut report);

        assert_eq!(resolved, case.expected, "{}", case.case);
        let unresolved: Vec<String> = report.unresolved.into_iter().collect();
        assert_eq!(unresolved, case.unresolved, "{}", case.case);
    }
}

#[test]
fn dynamic_values_are_evaluated_per_occurrence() {
    let mut report = Report::default();

    let text = resolve_text(
        "{{$uuid}} {{$uuid}}",
        &Scopes::default(),
        &SystemDynamics,
        &mut report,
    );

    let (first, second) = text.split_once(' ').unwrap();
    assert_ne!(first, second);
    assert!(report.unresolved.is_empty());
}

#[test]
fn a_secret_resolves_only_when_referenced_directly() {
    let scopes = global(&[
        ("token", Value::secret(Some("s3cr3t".into()))),
        ("auth", Value::plain("Bearer {{token}}")),
        ("base_url", Value::plain("https://evil/{{token}}")),
    ]);
    let mut report = Report::default();

    let text = resolve_text(
        "{{auth}} {{token}} {{base_url}}",
        &scopes,
        &FixedDynamics::default(),
        &mut report,
    );

    assert_eq!(text, "Bearer {{token}} s3cr3t https://evil/{{token}}");
    assert!(report.used_secret);
    assert_eq!(report.secret_values, ["s3cr3t"]);
    assert_eq!(report.secret_refs(), ["s3cr3t"]);
    let unresolved: Vec<&str> = report.unresolved.iter().map(String::as_str).collect();
    assert_eq!(unresolved, ["token"]);
}

#[test]
fn a_secret_value_is_taken_literally() {
    let scopes = global(&[
        ("password", Value::secret(Some("p{{x}}w\\{{y}}".into()))),
        ("x", Value::plain("X")),
    ]);
    let mut report = Report::default();

    let text = resolve_text(
        "{{password}}",
        &scopes,
        &FixedDynamics::default(),
        &mut report,
    );

    assert_eq!(text, "p{{x}}w\\{{y}}");
    assert!(report.unresolved.is_empty());
    assert_eq!(report.secret_values, ["p{{x}}w\\{{y}}"]);
}

#[test]
fn mask_mode_hides_direct_secrets_and_leaves_nested_ones_unresolved() {
    let scopes = global(&[
        ("token", Value::secret(Some("s3cr3t".into()))),
        ("auth", Value::plain("Bearer {{token}}")),
        ("host", Value::plain("example.com")),
    ]);
    let mut report = Report::default();

    let text = mask_text(
        "{{host}} {{auth}} {{token}} {{$uuid}} {{$nope}}",
        &scopes,
        &mut report,
    );

    assert_eq!(
        text,
        format!("example.com Bearer {{{{token}}}} {MASK} {{{{$uuid}}}} {{{{$nope}}}}")
    );
    assert!(!text.contains("s3cr3t"));
    let unresolved: Vec<&str> = report.unresolved.iter().map(String::as_str).collect();
    assert_eq!(unresolved, ["$nope", "token"]);
}

#[test]
fn an_expansion_bomb_stops_at_the_budget() {
    let fan_out = |next: &str| format!("{{{{{next}}}}}").repeat(100);
    let vars: Vec<(String, Value)> = (0..10)
        .map(|n| {
            let text = if n == 9 {
                "x".to_string()
            } else {
                fan_out(&format!("v{}", n + 1))
            };
            (format!("v{n}"), Value::plain(text))
        })
        .collect();
    let refs: Vec<(&str, Value)> = vars.iter().map(|(n, v)| (n.as_str(), v.clone())).collect();
    let scopes = global(&refs);
    let started = std::time::Instant::now();
    let mut report = Report::default();

    let text = resolve_text("{{v0}}", &scopes, &FixedDynamics::default(), &mut report);
    let mut masked = Report::default();
    let shown = mask_text("{{v0}}", &scopes, &mut masked);

    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    assert!(text.len() <= 2 * MAX_EXPANDED_BYTES, "{}", text.len());
    assert!(shown.len() <= 2 * MAX_EXPANDED_BYTES);
    assert!(!report.unresolved.is_empty());
}

#[test]
fn fixed_dynamics_supply_values() {
    let dynamics = FixedDynamics(BTreeMap::from([("$uuid".to_string(), "U".to_string())]));
    let mut report = Report::default();

    let text = resolve_text("{{$uuid}}", &Scopes::default(), &dynamics, &mut report);

    assert_eq!(text, "U");
}

#[test]
fn resolver_is_total_on_odd_inputs() {
    let alphabet = ['{', '}', '\\', ' ', 'a', '$', '\n', 'é', '🚀'];
    let scopes = global(&[("a", Value::plain("{{a}}{{")), ("", Value::plain("x"))]);
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    for _ in 0..2_000 {
        let length = (seed % 24) as usize;
        let text: String = (0..length)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                alphabet[(seed % alphabet.len() as u64) as usize]
            })
            .collect();
        let mut report = Report::default();
        let _ = resolve_text(&text, &scopes, &SystemDynamics, &mut report);
        let _ = mask_text(&text, &scopes, &mut report);
    }
}
