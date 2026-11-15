//! Import -> export -> re-import equivalence (plan 09 §10, risk "round trip not equivalent").
//!
//! Equivalence is defined on the mappable subset. One lossy spot, by design: the v2.1 schema
//! rejects `type: "secret"` on collection variables, so a collection-level secret variable is
//! exported as an empty string with `type: "string"`. The secret flag comes back only through
//! the importer's sensitive-name check: the preview offers the variable as a candidate and the
//! user (here: `secret_names = secret_candidates`) ticks it again. Environment and globals keep
//! `type: "secret"` and need no such help.

#[path = "support/postman.rs"]
mod support;

use std::collections::BTreeMap;

use papan_lib::interop::postman::flow::plan_paths;
use papan_lib::interop::postman::types::{ImportOptions, ReportItem};
use support::{export_options, fixture, import_options, snapshot, World};

/// `v21/scripts` is left out on purpose: imported scripts are switched off, they export as
/// `disabled: true` events, and a disabled event is dropped on import (so the second pass has
/// no scripts). `postman_review.rs` covers that path.
const FIXTURES: [&str; 6] = [
    "v21/basic.postman_collection.json",
    "v21/bodies.postman_collection.json",
    "v21/auth.postman_collection.json",
    "v21/variables.postman_collection.json",
    "v21/secrets.postman_collection.json",
    "v21/misc.postman_collection.json",
];

struct Pass {
    tree: BTreeMap<String, Vec<u8>>,
    export: Vec<u8>,
    report: Vec<ReportItem>,
}

/// Exports collection `id`, returning its bytes and the export report.
fn export_of(
    world: &World,
    id: &str,
) -> (
    Vec<u8>,
    Vec<ReportItem>,
    std::path::PathBuf,
    tempfile::TempDir,
) {
    let (out, result) = world.export(&export_options(id)).unwrap();
    let path = out.path().join(&result.files[0]);
    (std::fs::read(&path).unwrap(), result.report, path, out)
}

/// Pass 1 imports the fixture; pass 2 imports pass 1's export (with the user re-ticking the
/// secret candidates); both are exported again.
fn two_passes(file: &str) -> (Pass, Pass) {
    let mut world = World::new();
    let (plan0, _) = plan_paths(&[fixture(file)]).unwrap();
    let first = ImportOptions {
        secret_names: plan0.summaries[0].secret_candidates.clone(),
        ..import_options(false)
    };
    let id1 = world
        .import_new(&[file], &first)
        .unwrap()
        .collection_id
        .unwrap();
    let (export1, report1, exported_path, _keep) = export_of(&world, &id1);
    let (plan, _) = plan_paths(std::slice::from_ref(&exported_path)).unwrap();
    let second = ImportOptions {
        secret_names: plan.summaries[0].secret_candidates.clone(),
        ..first
    };
    let id2 = world
        .import_paths(&[exported_path], &second)
        .unwrap()
        .collection_id
        .unwrap();
    let (export2, report2, _, _keep2) = export_of(&world, &id2);
    let pass1 = Pass {
        tree: snapshot(&world.root(&id1)),
        export: export1,
        report: report1,
    };
    let pass2 = Pass {
        tree: snapshot(&world.root(&id2)),
        export: export2,
        report: report2,
    };
    (pass1, pass2)
}

#[test]
fn import_export_import_gives_equal_trees() {
    for file in FIXTURES {
        let (first, second) = two_passes(file);

        let paths: Vec<_> = first.tree.keys().collect();
        assert_eq!(
            paths,
            second.tree.keys().collect::<Vec<_>>(),
            "{file}: files"
        );
        for (path, bytes) in &first.tree {
            assert_eq!(
                String::from_utf8_lossy(bytes),
                String::from_utf8_lossy(&second.tree[path]),
                "{file}: {path}"
            );
        }
    }
}

#[test]
fn export_import_export_gives_equal_bytes_and_reports() {
    for file in FIXTURES {
        let (first, second) = two_passes(file);

        assert_eq!(
            String::from_utf8_lossy(&first.export),
            String::from_utf8_lossy(&second.export),
            "{file}"
        );
        assert_eq!(first.report, second.report, "{file}");
    }
}

#[test]
fn a_collection_secret_survives_only_through_the_sensitive_name_check() {
    let file = "v21/secrets.postman_collection.json";
    let mut world = World::new();
    let options = ImportOptions {
        secret_names: vec!["password".into()],
        ..import_options(false)
    };
    let id = world
        .import_new(&[file], &options)
        .unwrap()
        .collection_id
        .unwrap();
    let (export, _, path, _keep) = export_of(&world, &id);
    let text = String::from_utf8(export).unwrap();
    assert!(text.contains("\"key\": \"password\""));
    assert!(
        !text.contains("\"type\": \"secret\""),
        "v2.1 collection variables cannot be secret"
    );

    let untouched = world
        .import_paths(std::slice::from_ref(&path), &import_options(false))
        .unwrap();
    let ticked = {
        let (plan, _) = plan_paths(std::slice::from_ref(&path)).unwrap();
        assert!(plan.summaries[0]
            .secret_candidates
            .contains(&"password".to_string()));
        let again = ImportOptions {
            secret_names: vec!["password".into()],
            ..import_options(false)
        };
        world.import_paths(&[path], &again).unwrap()
    };

    let plain = world
        .store
        .collection_variables(&untouched.collection_id.unwrap())
        .unwrap();
    let secret = world
        .store
        .collection_variables(&ticked.collection_id.unwrap())
        .unwrap();
    assert!(!plain["password"].secret, "no tick, no secret flag");
    assert!(secret["password"].secret);
}
