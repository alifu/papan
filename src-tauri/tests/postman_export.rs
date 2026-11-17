//! Postman export of imported fixtures (plan 09 §10): v2.1 schema validity, secrets, names.

#[path = "support/postman.rs"]
mod support;

use papan_lib::interop::postman::export_flow::{build_export, write_bundle};
use papan_lib::interop::postman::flow::cap_items;
use papan_lib::interop::postman::types::{ExportOptions, ImportOptions, ReportItem, ReportLevel};
use serde_json::Value;
use support::{any_contains, export_options, fixture, import_options, snapshot, World};

const COLLECTIONS: [&str; 7] = [
    "v21/basic.postman_collection.json",
    "v21/bodies.postman_collection.json",
    "v21/auth.postman_collection.json",
    "v21/scripts.postman_collection.json",
    "v21/variables.postman_collection.json",
    "v21/secrets.postman_collection.json",
    "v21/misc.postman_collection.json",
];
const ENV_SECRET: &str = "FAKE-ENV-API-KEY-DO-NOT-USE";
const GLOBAL_SECRET: &str = "FAKE-GLOBAL-API-KEY-DO-NOT-USE";
const PASSWORD: &str = "FAKE-PASSWORD-DO-NOT-USE";
const BASIC: &str = "FAKE-BASIC-DO-NOT-USE";

fn assert_valid_v21(bytes: &[u8]) {
    let schema: Value =
        serde_json::from_slice(&std::fs::read(fixture("schema/collection-v2.1.0.json")).unwrap())
            .unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let instance: Value = serde_json::from_slice(bytes).unwrap();
    let errors: Vec<String> = validator
        .iter_errors(&instance)
        .map(|e| format!("{e} at {}", e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
}

fn full_world(store_secrets: bool) -> (World, String) {
    let mut world = World::new();
    let options = ImportOptions {
        secret_names: vec!["password".into()],
        ..import_options(store_secrets)
    };
    let result = world
        .import_new(
            &[
                "v21/secrets.postman_collection.json",
                "env/dev.postman_environment.json",
                "env/globals.postman_globals.json",
            ],
            &options,
        )
        .unwrap();
    let id = result.collection_id.unwrap();
    (world, id)
}

#[test]
fn every_fixture_exports_a_valid_v21_collection() {
    for file in COLLECTIONS {
        let mut world = World::new();
        let id = world
            .import_new(&[file], &import_options(false))
            .unwrap()
            .collection_id
            .unwrap();

        let (out, result) = world.export(&export_options(&id)).unwrap();

        assert_eq!(result.files.len(), 1, "{file}");
        assert!(result.files[0].ends_with(".postman_collection.json"));
        assert_valid_v21(&std::fs::read(out.path().join(&result.files[0])).unwrap());
    }
}

#[test]
fn environments_and_globals_are_exported_when_asked() {
    let (world, id) = full_world(false);
    let options = ExportOptions {
        environments: vec!["dev".into()],
        include_globals: true,
        ..export_options(&id)
    };

    let (out, result) = world.export(&options).unwrap();

    assert_eq!(
        result.files,
        [
            "secrets-v2-1.postman_collection.json",
            "dev.postman_environment.json",
            "globals.postman_globals.json"
        ]
    );
    let env: Value =
        serde_json::from_slice(&std::fs::read(out.path().join(&result.files[1])).unwrap()).unwrap();
    assert_eq!(env["_postman_variable_scope"], "environment");
    assert_eq!(env["values"].as_array().unwrap().len(), 4);
}

#[test]
fn secret_values_are_in_no_exported_byte_unless_asked_for() {
    let (world, id) = full_world(true);
    let options = ExportOptions {
        environments: vec!["dev".into()],
        include_globals: true,
        ..export_options(&id)
    };

    let (out, result) = world.export(&options).unwrap();

    let files = snapshot(out.path());
    for secret in [ENV_SECRET, GLOBAL_SECRET, PASSWORD, BASIC] {
        assert!(!any_contains(&files, secret), "{secret} exported");
        assert!(!format!("{:?}", result.report).contains(secret));
    }
}

#[test]
fn include_secret_values_reads_the_vault_into_the_files() {
    let (world, id) = full_world(true);
    let options = ExportOptions {
        environments: vec!["dev".into()],
        include_globals: true,
        include_secret_values: true,
        ..export_options(&id)
    };

    let (out, _) = world.export(&options).unwrap();

    let files = snapshot(out.path());
    assert!(any_contains(&files, ENV_SECRET));
    assert!(any_contains(&files, GLOBAL_SECRET));
    assert!(any_contains(&files, PASSWORD));
}

#[test]
fn an_existing_file_name_is_never_overwritten() {
    let (world, id) = full_world(false);
    let out = tempfile::tempdir().unwrap();
    let options = export_options(&id);
    let first = write_bundle(
        out.path(),
        build_export(&world.store, world.vault.as_ref(), &options).unwrap(),
    )
    .unwrap();
    let before = std::fs::read(out.path().join(&first.files[0])).unwrap();

    let second = write_bundle(
        out.path(),
        build_export(&world.store, world.vault.as_ref(), &options).unwrap(),
    )
    .unwrap();
    let third = write_bundle(
        out.path(),
        build_export(&world.store, world.vault.as_ref(), &options).unwrap(),
    )
    .unwrap();

    assert_eq!(second.files, ["secrets-v2-1 (2).postman_collection.json"]);
    assert_eq!(third.files, ["secrets-v2-1 (3).postman_collection.json"]);
    assert_eq!(
        std::fs::read(out.path().join(&first.files[0])).unwrap(),
        before
    );
}

#[test]
fn writing_into_a_missing_folder_fails_without_creating_it() {
    let (world, id) = full_world(false);
    let missing = world.work.path().join("nope");

    let bundle = build_export(&world.store, world.vault.as_ref(), &export_options(&id)).unwrap();

    assert!(write_bundle(&missing, bundle).is_err());
    assert!(!missing.exists());
}

#[test]
fn an_unknown_environment_fails_before_anything_is_written() {
    let (world, id) = full_world(false);
    let options = ExportOptions {
        environments: vec!["missing".into()],
        ..export_options(&id)
    };

    assert!(world.export(&options).is_err());
}

#[test]
fn the_report_is_capped_at_a_thousand_entries() {
    let items: Vec<ReportItem> = (0..1005)
        .map(|n| ReportItem {
            level: ReportLevel::Info,
            file: "a".into(),
            path: n.to_string(),
            message: "x".into(),
        })
        .collect();

    let capped = cap_items(items);

    assert_eq!(capped.len(), 1001);
    assert_eq!(capped[1000].message, "and 5 more.");
}
