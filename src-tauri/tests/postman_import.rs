//! Postman import against the fixtures (plan 09 §10): trees, scripts, environments,
//! globals, secrets and the commit targets.

#[path = "support/postman.rs"]
mod support;

use papan_lib::error::ErrorCode;
use papan_lib::interop::postman::flow::plan_paths;
use papan_lib::interop::postman::types::{GlobalsClash, ImportOptions, ImportTarget};
use papan_lib::store::env_model::VarEntry;
use papan_lib::store::types::{RequestLocation, ScriptTarget};
use papan_lib::vars::vault::SecretStore;
use support::{any_contains, fixture, import_options, requests, snapshot, World};

const COLLECTIONS: [&str; 9] = [
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
const ENV_SECRET: &str = "FAKE-ENV-API-KEY-DO-NOT-USE";
const GLOBAL_SECRET: &str = "FAKE-GLOBAL-API-KEY-DO-NOT-USE";
const COLLECTION_SECRETS: [&str; 3] = [
    "FAKE-PASSWORD-DO-NOT-USE",
    "FAKE-TOKEN-DO-NOT-USE",
    "FAKE-BASIC-DO-NOT-USE",
];

#[test]
fn every_fixture_collection_imports_with_the_counts_of_its_preview() {
    for file in COLLECTIONS {
        let mut world = World::new();
        let (plan, _) = plan_paths(&[fixture(file)]).unwrap();
        let summary = plan.summaries[0].clone();

        let result = world.import_new(&[file], &import_options(false)).unwrap();

        let id = result.collection_id.unwrap();
        let tree = world.tree(&id);
        assert!(tree.collection.available, "{file}");
        assert_eq!(
            requests(&tree.children).len() as u32,
            summary.requests,
            "{file}"
        );
        assert_eq!(tree.collection.name, summary.name, "{file}");
    }
}

#[test]
fn imported_scripts_are_switched_off_and_the_report_says_so() {
    let mut world = World::new();

    let result = world
        .import_new(
            &["v21/scripts.postman_collection.json"],
            &import_options(false),
        )
        .unwrap();

    let id = result.collection_id.unwrap();
    let collection = world
        .store
        .read_scripts(&ScriptTarget::Collection {
            collection_id: id.clone(),
        })
        .unwrap();
    assert!(!collection.enabled);
    assert!(collection.pre_request.contains("pm.environment.set"));
    let folder = world
        .store
        .read_scripts(&ScriptTarget::Folder {
            collection_id: id.clone(),
            path: "folder-with-script".into(),
        })
        .unwrap();
    assert!(!folder.enabled && !folder.post_response.is_empty());
    for node in requests(&world.tree(&id).children) {
        let location = RequestLocation::Collection {
            collection_id: id.clone(),
            path: node.path.clone(),
        };
        let doc = world.store.read_request(&location).unwrap();
        assert!(!doc.content.scripts.enabled, "{}", node.path);
    }
    assert!(result
        .report
        .iter()
        .any(|item| item.message.contains("switched off")));
}

#[test]
fn leaving_scripts_out_imports_none() {
    let mut world = World::new();
    let options = ImportOptions {
        import_scripts: false,
        ..import_options(false)
    };

    let result = world
        .import_new(&["v21/scripts.postman_collection.json"], &options)
        .unwrap();

    let id = result.collection_id.unwrap();
    let scripts = world
        .store
        .read_scripts(&ScriptTarget::Collection { collection_id: id })
        .unwrap();
    assert!(scripts.pre_request.is_empty() && scripts.post_response.is_empty());
}

#[test]
fn environments_land_in_the_collection_and_secret_values_stay_out_of_the_files() {
    let mut world = World::new();

    let result = world
        .import_new(
            &[
                "v21/basic.postman_collection.json",
                "env/dev.postman_environment.json",
            ],
            &import_options(false),
        )
        .unwrap();

    let id = result.collection_id.unwrap();
    assert_eq!(result.environments, ["dev"]);
    let env = world.store.read_environment(&id, "dev").unwrap();
    assert!(env.variables["apiKey"].secret);
    assert!(!env.variables["oldHost"].enabled);
    assert!(!any_contains(&snapshot(&world.root(&id)), ENV_SECRET));
    assert!(!world.vault.has(&format!("e/{id}/dev/apiKey")).unwrap());
}

#[test]
fn storing_secret_values_puts_them_in_the_vault_only() {
    let mut world = World::new();

    let result = world
        .import_new(
            &[
                "v21/secrets.postman_collection.json",
                "env/dev.postman_environment.json",
                "env/globals.postman_globals.json",
            ],
            &ImportOptions {
                secret_names: vec!["password".into()],
                ..import_options(true)
            },
        )
        .unwrap();

    let id = result.collection_id.unwrap();
    assert_eq!(
        world
            .vault
            .get(&format!("e/{id}/dev/apiKey"))
            .unwrap()
            .as_deref(),
        Some(ENV_SECRET)
    );
    assert_eq!(
        world.vault.get("g/globalApiKey").unwrap().as_deref(),
        Some(GLOBAL_SECRET)
    );
    let files = snapshot(&world.root(&id));
    let app = snapshot(world.app.path());
    for secret in COLLECTION_SECRETS
        .iter()
        .chain([&ENV_SECRET, &GLOBAL_SECRET])
    {
        let hit: Vec<_> = files
            .iter()
            .filter(|(_, b)| String::from_utf8_lossy(b).contains(*secret))
            .map(|(k, _)| k.clone())
            .collect();
        assert!(hit.is_empty(), "{secret} in {hit:?}");
        assert!(!any_contains(&app, secret), "{secret} in app data");
        let report = format!("{:?}", result.report);
        assert!(!report.contains(secret), "{secret} in the report");
    }
}

#[test]
fn globals_clash_keeps_or_overwrites_existing_values() {
    for (clash, expected) in [
        (GlobalsClash::Keep, "mine"),
        (GlobalsClash::Overwrite, "papan-fixture"),
    ] {
        let mut world = World::new();
        let mut table = std::collections::BTreeMap::new();
        table.insert(
            "userAgent".to_string(),
            VarEntry::new("mine".into(), true, false),
        );
        world.store.write_globals(table).unwrap();
        let options = ImportOptions {
            globals_clash: clash,
            ..import_options(false)
        };

        let result = world
            .import_new(
                &[
                    "v21/basic.postman_collection.json",
                    "env/globals.postman_globals.json",
                ],
                &options,
            )
            .unwrap();

        let globals = world.store.globals().unwrap().variables;
        assert_eq!(globals["userAgent"].value, expected, "{clash:?}");
        assert!(globals.contains_key("globalApiKey"), "{clash:?}");
        assert!(result.globals > 0, "{clash:?}");
    }
}

#[test]
fn importing_into_an_open_collection_adds_a_folder_and_environments() {
    let mut world = World::new();
    let tree = world
        .store
        .create_collection(world.work.path(), "Mine")
        .unwrap();
    let id = tree.collection.id;
    let options = ImportOptions {
        target: ImportTarget::Collection {
            collection_id: id.clone(),
        },
        ..import_options(false)
    };

    let result = world
        .import_new(
            &[
                "v21/basic.postman_collection.json",
                "env/dev.postman_environment.json",
            ],
            &options,
        )
        .unwrap();

    assert_eq!(result.collection_id.as_deref(), Some(id.as_str()));
    assert_eq!(result.environments, ["dev"]);
    let after = world.tree(&id);
    assert_eq!(after.children.len(), 1);
    assert!(!requests(&after.children).is_empty());
}

#[test]
fn an_environment_only_import_needs_a_collection_and_writes_nothing() {
    let mut world = World::new();

    let error = world
        .import_new(
            &["env/dev.postman_environment.json"],
            &import_options(false),
        )
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert_eq!(std::fs::read_dir(world.work.path()).unwrap().count(), 0);
}

#[test]
fn a_failed_commit_leaves_no_partial_result() {
    let mut world = World::new();
    let options = ImportOptions {
        target: ImportTarget::Collection {
            collection_id: "missing".into(),
        },
        ..import_options(false)
    };

    let error = world
        .import_new(&["v21/basic.postman_collection.json"], &options)
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(std::fs::read_dir(world.work.path()).unwrap().count(), 0);
    assert!(world.store.list_collections().is_empty());
}

#[test]
fn two_imports_of_the_same_collection_get_distinct_folders() {
    let mut world = World::new();

    let first = world
        .import_new(
            &["v21/basic.postman_collection.json"],
            &import_options(false),
        )
        .unwrap();
    let second = world
        .import_new(
            &["v21/basic.postman_collection.json"],
            &import_options(false),
        )
        .unwrap();

    assert_ne!(first.collection_id, second.collection_id);
    assert_eq!(std::fs::read_dir(world.work.path()).unwrap().count(), 2);
}
