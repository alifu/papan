use std::path::PathBuf;

use super::*;
use crate::interop::postman::types::{GlobalsClash, ImportOptions, ImportTarget};

fn fixture(rel: &str) -> (String, Vec<u8>) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/postman")
        .join(rel);
    let name = limits::file_label(&path);
    (name, std::fs::read(path).unwrap())
}

fn options(secret_names: &[&str], import_scripts: bool) -> ImportOptions {
    ImportOptions {
        target: ImportTarget::NewCollection,
        secret_names: secret_names.iter().map(|s| (*s).to_string()).collect(),
        store_secret_values: false,
        import_scripts,
        globals_clash: GlobalsClash::Keep,
    }
}

#[test]
fn every_good_fixture_plans_without_error() {
    let files = [
        "v21/basic.postman_collection.json",
        "v21/bodies.postman_collection.json",
        "v21/auth.postman_collection.json",
        "v21/scripts.postman_collection.json",
        "v21/variables.postman_collection.json",
        "v21/secrets.postman_collection.json",
        "v21/misc.postman_collection.json",
        "v20/basic.postman_collection.json",
        "v20/auth.postman_collection.json",
    ];
    for rel in files {
        let (plan, _) = plan_files(&[fixture(rel)]).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let collection = plan.collection.unwrap();
        assert!(collection.requests > 0, "{rel}");
    }
}

#[test]
fn collection_with_environment_and_globals_keeps_secret_values_out_of_tables() {
    let files = [
        fixture("v21/basic.postman_collection.json"),
        fixture("env/dev.postman_environment.json"),
        fixture("env/globals.postman_globals.json"),
    ];

    let (plan, _) = plan_files(&files).unwrap();

    assert_eq!(plan.summaries.len(), 3);
    let env = &plan.environments[0];
    assert_eq!(env.name, "dev");
    assert!(env.vars.table["apiKey"].secret);
    assert_eq!(env.vars.table["apiKey"].value, "");
    assert_eq!(
        env.vars.secrets.get("apiKey"),
        Some("FAKE-ENV-API-KEY-DO-NOT-USE")
    );
    assert!(!env.vars.table["oldHost"].enabled);
    let globals = plan.globals.as_ref().unwrap();
    assert!(globals.table["globalApiKey"].secret);
    let debug = format!("{plan:?}");
    assert!(!debug.contains("FAKE-ENV-API-KEY"), "secret value in Debug");
    assert!(
        !debug.contains("FAKE-GLOBAL-API-KEY"),
        "secret value in Debug"
    );
}

#[test]
fn scripts_headline_comes_first() {
    let (plan, report) = plan_files(&[fixture("v21/scripts.postman_collection.json")]).unwrap();

    assert!(plan.scripts() > 0);
    assert!(report.items()[0]
        .message
        .contains("imported but switched off"));
}

#[test]
fn hostile_fixtures_fail_cleanly_or_skip() {
    for rel in [
        "hostile/bad-deep-nesting.postman_collection.json",
        "hostile/bad-wrong-types.postman_collection.json",
        "hostile/bad-env-duplicate-keys.postman_environment.json",
    ] {
        let (name, bytes) = fixture(rel);
        let error = plan_files(&[(name.clone(), bytes)]).unwrap_err();
        assert!(
            error.message.starts_with(&format!("{name}: ")),
            "{}",
            error.message
        );
    }
    for rel in [
        "hostile/bad-duplicate-names.postman_collection.json",
        "hostile/bad-path-names.postman_collection.json",
    ] {
        assert!(plan_files(&[fixture(rel)]).is_ok(), "{rel}");
    }
}

#[test]
fn file_level_refusals() {
    let two = [
        fixture("v21/basic.postman_collection.json"),
        fixture("v21/auth.postman_collection.json"),
    ];
    let many: Vec<(String, Vec<u8>)> = (0..21)
        .map(|n| (format!("{n}.json"), b"{}".to_vec()))
        .collect();

    assert!(plan_files(&two)
        .unwrap_err()
        .message
        .contains("one collection"));
    assert!(plan_files(&many).is_err());
    assert!(plan_files(&[("x.json".into(), b"[]".to_vec())]).is_err());
    assert!(plan_files(&[("x.json".into(), b"\xff".to_vec())]).is_err());
}

#[test]
fn options_make_chosen_names_secret_and_can_drop_scripts() {
    let (plan, _) = plan_files(&[
        fixture("v21/scripts.postman_collection.json"),
        fixture("env/dev.postman_environment.json"),
    ])
    .unwrap();

    let chosen = with_options(&plan, &options(&["baseUrl"], false));

    let env = &chosen.environments[0];
    assert!(env.vars.table["baseUrl"].secret);
    assert_eq!(
        env.vars.secrets.get("baseUrl"),
        Some("https://api.example.test")
    );
    let collection = chosen.collection.unwrap();
    assert_eq!(collection.scripts, 0);
    assert!(!collection.file.extra.contains_key("scripts"));
    let kept = with_options(&plan, &options(&[], true));
    assert_eq!(kept.collection.unwrap().scripts, plan.scripts());
}
