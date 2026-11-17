use super::*;
use crate::interop::postman::import::secrets::SecretValues;
use crate::interop::postman::import::vars::PlannedVars;

fn plan() -> ImportPlan {
    ImportPlan::default()
}

#[test]
fn stage_returns_a_preview_with_a_uuid_token_and_get_finds_it() {
    let mut stages = Stages::default();
    let now = Instant::now();

    let preview = stages.stage(plan(), Report::new(), now);

    assert!(uuid::Uuid::parse_str(&preview.token).is_ok());
    assert!(stages.get(&preview.token, now).is_ok());
    assert!(
        stages.get(&preview.token, now).is_ok(),
        "get keeps the stage"
    );
}

#[test]
fn an_unknown_token_is_not_found() {
    let mut stages = Stages::default();

    let error = stages.get("nope", Instant::now()).unwrap_err();

    assert_eq!(error.code, crate::error::ErrorCode::NotFound);
}

#[test]
fn a_stage_expires_after_the_ttl() {
    let mut stages = Stages::default();
    let start = Instant::now();
    let preview = stages.stage(plan(), Report::new(), start);

    assert!(stages
        .get(&preview.token, start + STAGE_TTL - Duration::from_secs(1))
        .is_ok());
    assert!(stages.get(&preview.token, start + STAGE_TTL).is_err());
    assert!(stages.is_empty());
}

#[test]
fn the_oldest_stage_is_dropped_past_the_limit() {
    let mut stages = Stages::default();
    let now = Instant::now();
    let tokens: Vec<String> = (0..=MAX_STAGED)
        .map(|_| stages.stage(plan(), Report::new(), now).token)
        .collect();

    assert_eq!(stages.len(), MAX_STAGED);
    assert!(stages.get(&tokens[0], now).is_err());
    assert!(stages.get(&tokens[MAX_STAGED], now).is_ok());
}

#[test]
fn discard_drops_the_stage_and_ignores_unknown_tokens() {
    let mut stages = Stages::default();
    let now = Instant::now();
    let preview = stages.stage(plan(), Report::new(), now);

    stages.discard("unknown");
    stages.discard(&preview.token);

    assert!(stages.is_empty());
}

#[test]
fn the_preview_carries_the_report_and_its_truncated_count() {
    let mut stages = Stages::default();
    let mut report = Report::new();
    for _ in 0..crate::interop::postman::report::MAX_REPORT_ITEMS + 3 {
        report.info("a.json", "", "x");
    }

    let preview = stages.stage(plan(), report, Instant::now());

    assert_eq!(preview.report.len(), 1000);
    assert_eq!(preview.truncated, 3);
}

#[test]
fn debug_of_staged_data_never_prints_secret_values() {
    let mut secrets = SecretValues::default();
    secrets.insert("token", "SUPER-SECRET-VALUE");
    let mut table = crate::store::env_model::VarTable::new();
    table.insert(
        "password".into(),
        crate::store::env_model::VarEntry::new("PLAIN-LOOKS-SENSITIVE".into(), true, false),
    );
    let vars = PlannedVars {
        table,
        secrets,
        ..PlannedVars::default()
    };
    let staged = ImportPlan {
        globals: Some(vars),
        ..ImportPlan::default()
    };
    let mut stages = Stages::default();
    stages.stage(staged, Report::new(), Instant::now());

    let shown = format!("{stages:?}");

    assert!(!shown.contains("SUPER-SECRET-VALUE"));
    assert!(!shown.contains("PLAIN-LOOKS-SENSITIVE"));
}

#[test]
fn a_taken_stage_cannot_be_taken_twice_and_comes_back_when_put_back() {
    let mut stages = Stages::default();
    let now = Instant::now();
    let preview = stages.stage(plan(), Report::new(), now);

    let taken = stages.take(&preview.token, now).unwrap();

    assert!(stages.take(&preview.token, now).is_err());
    stages.put_back(taken);
    assert!(stages.take(&preview.token, now).is_ok());
}
