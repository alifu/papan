//! Review fixes (plan 09): paths never imported, clashing lifted names, disabled scripts,
//! environment alignment, deterministic exports, URL and form credentials.

#[path = "support/postman.rs"]
mod support;

use std::collections::BTreeMap;

use papan_lib::interop::postman::export_flow::build_export;
use papan_lib::interop::postman::flow::plan_paths;
use papan_lib::interop::postman::types::{ExportOptions, ImportOptions, ImportTarget, ReportLevel};
use papan_lib::store::env_model::VarEntry;
use papan_lib::store::types::{Body, RequestLocation, ScriptTarget, Scripts};
use papan_lib::vars::vault::SecretStore;
use serde_json::{json, Value};
use support::{any_contains, export_options, import_options, requests, snapshot, World};

const SCHEMA: &str = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

fn temp_json(dir: &tempfile::TempDir, name: &str, value: &Value) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}

fn into(id: &str) -> ImportOptions {
    ImportOptions {
        target: ImportTarget::Collection {
            collection_id: id.into(),
        },
        ..import_options(false)
    }
}

#[test]
fn imported_requests_never_carry_a_file_path() {
    let dir = tempfile::tempdir().unwrap();
    let doc = json!({"info": {"name": "Files", "schema": SCHEMA}, "item": [
        {"name": "up", "request": {"method": "POST", "url": "http://a.test", "body": {"mode": "formdata",
            "formdata": [{"key": "f", "type": "file", "src": "/home/x/.ssh/id_rsa"}]}}},
        {"name": "bin", "request": {"method": "POST", "url": "http://a.test", "body": {"mode": "file",
            "file": {"src": "/etc/passwd"}}}}]});
    let path = temp_json(&dir, "f.postman_collection.json", &doc);
    let mut world = World::new();

    let (_, plan_report) = plan_paths(std::slice::from_ref(&path)).unwrap();
    let result = world.import_paths(&[path], &import_options(false)).unwrap();

    let id = result.collection_id.unwrap();
    assert!(!any_contains(&snapshot(&world.root(&id)), "id_rsa"));
    assert!(!any_contains(&snapshot(&world.root(&id)), "/etc/passwd"));
    for node in requests(&world.tree(&id).children) {
        let location = RequestLocation::Collection {
            collection_id: id.clone(),
            path: node.path.clone(),
        };
        let body = world.store.read_request(&location).unwrap().content.body;
        assert!(!matches!(body, Body::Binary { file: Some(_) }));
        if let Body::Multipart { parts } = body {
            assert!(parts.iter().all(|p| p.file.is_none()));
        }
    }
    assert!(
        plan_report
            .items()
            .iter()
            .any(|i| i.message.contains("id_rsa")),
        "the report keeps the path text"
    );
}

#[test]
fn a_lifted_name_that_clashes_with_the_target_is_renamed() {
    let mut world = World::new();
    let id = world
        .store
        .create_collection(world.work.path(), "Mine")
        .unwrap()
        .collection
        .id;
    let mut vars = BTreeMap::new();
    vars.insert(
        "literal_basic_credentials_password".to_string(),
        VarEntry::new("unrelated".into(), true, false),
    );
    world.store.write_collection_variables(&id, vars).unwrap();

    let result = world
        .import_new(&["v21/secrets.postman_collection.json"], &into(&id))
        .unwrap();

    let after = world.store.collection_variables(&id).unwrap();
    assert_eq!(
        after["literal_basic_credentials_password"].value,
        "unrelated"
    );
    assert!(after["literal_basic_credentials_password_2"].secret);
    let files = snapshot(&world.root(&id));
    assert!(any_contains(
        &files,
        "{{literal_basic_credentials_password_2}}"
    ));
    assert!(result
        .report
        .iter()
        .any(|i| i.message.contains("literal_basic_credentials_password_2")));
}

#[test]
fn the_postman_id_is_not_copied_into_a_folder_file() {
    let mut world = World::new();
    let id = world
        .store
        .create_collection(world.work.path(), "Mine")
        .unwrap()
        .collection
        .id;

    world
        .import_new(&["v21/basic.postman_collection.json"], &into(&id))
        .unwrap();

    let files = snapshot(&world.root(&id));
    assert!(!files
        .iter()
        .filter(|(k, _)| k.ends_with("_folder.yaml"))
        .any(|(_, v)| any_contains(
            &BTreeMap::from([("x".to_string(), v.clone())]),
            "postman_id"
        )));
}

#[test]
fn scripts_export_disabled_unless_enabled_in_papan_and_disabled_events_are_dropped() {
    let mut world = World::new();
    let id = world
        .import_new(
            &["v21/scripts.postman_collection.json"],
            &import_options(false),
        )
        .unwrap()
        .collection_id
        .unwrap();
    let (out, result) = world.export(&export_options(&id)).unwrap();
    let text = std::fs::read_to_string(out.path().join(&result.files[0])).unwrap();
    assert!(
        text.contains("\"disabled\": true"),
        "imported scripts stay switched off"
    );
    let target = ScriptTarget::Collection {
        collection_id: id.clone(),
    };
    world
        .store
        .write_scripts(
            &target,
            &Scripts {
                enabled: true,
                pre_request: "pm.test('x');".into(),
                post_response: String::new(),
            },
        )
        .unwrap();

    let (out, result) = world.export(&export_options(&id)).unwrap();

    let doc: Value =
        serde_json::from_slice(&std::fs::read(out.path().join(&result.files[0])).unwrap()).unwrap();
    let event = &doc["event"][0];
    assert_eq!(event["script"]["exec"][0], "pm.test('x');");
    assert!(
        event.get("disabled").is_none(),
        "an enabled script is written enabled"
    );

    let dir = tempfile::tempdir().unwrap();
    let off = json!({"info": {"name": "Off", "schema": SCHEMA}, "event": [
        {"listen": "prerequest", "disabled": true, "script": {"exec": ["SECRET_OFF();"]}}], "item": []});
    let path = temp_json(&dir, "o.postman_collection.json", &off);
    let (_, plan_report) = plan_paths(std::slice::from_ref(&path)).unwrap();
    let imported = world.import_paths(&[path], &import_options(false)).unwrap();
    let scripts = world
        .store
        .read_scripts(&ScriptTarget::Collection {
            collection_id: imported.collection_id.unwrap(),
        })
        .unwrap();
    assert!(scripts.pre_request.is_empty());
    assert!(plan_report
        .items()
        .iter()
        .any(|i| i.message.contains("disabled script")));
}

#[test]
fn environments_over_the_room_keep_their_own_names_and_secrets() {
    let mut world = World::new();
    let id = world
        .store
        .create_collection(world.work.path(), "Mine")
        .unwrap()
        .collection
        .id;
    for n in 0..48 {
        world
            .store
            .create_environment(&id, &format!("e{n}"))
            .unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let paths: Vec<_> = ["alpha", "beta", "gamma"]
        .iter()
        .map(|name| {
            let doc = json!({"name": name, "values": [{"key": "token", "value": format!("SECRET-{name}"), "type": "secret", "enabled": true}],
                "_postman_variable_scope": "environment"});
            temp_json(&dir, &format!("{name}.postman_environment.json"), &doc)
        })
        .collect();
    let options = ImportOptions {
        store_secret_values: true,
        ..into(&id)
    };

    let result = world.import_paths(&paths, &options).unwrap();

    assert_eq!(result.environments, ["alpha", "beta"]);
    for name in ["alpha", "beta"] {
        let stored = world.vault.get(&format!("e/{id}/{name}/token")).unwrap();
        assert_eq!(stored.as_deref(), Some(format!("SECRET-{name}").as_str()));
    }
    assert!(!world.vault.has(&format!("e/{id}/gamma/token")).unwrap());
}

#[test]
fn exports_of_environments_and_globals_are_byte_stable() {
    let mut world = World::new();
    let id = world
        .import_new(
            &[
                "v21/basic.postman_collection.json",
                "env/dev.postman_environment.json",
                "env/globals.postman_globals.json",
            ],
            &import_options(false),
        )
        .unwrap()
        .collection_id
        .unwrap();
    let options = ExportOptions {
        environments: vec!["dev".into()],
        include_globals: true,
        ..export_options(&id)
    };

    let (first, _) = world.export(&options).unwrap();
    let (second, _) = world.export(&options).unwrap();

    assert_eq!(snapshot(first.path()), snapshot(second.path()));
}

#[test]
fn included_secret_values_warn_and_never_show_in_debug() {
    let mut world = World::new();
    let options = ImportOptions {
        secret_names: vec!["password".into()],
        ..import_options(true)
    };
    let id = world
        .import_new(&["v21/secrets.postman_collection.json"], &options)
        .unwrap()
        .collection_id
        .unwrap();
    let export = ExportOptions {
        include_secret_values: true,
        ..export_options(&id)
    };

    let bundle = build_export(&world.store, world.vault.as_ref(), &export).unwrap();
    let (_, result) = world.export(&export).unwrap();

    assert!(!format!("{bundle:?}").contains("FAKE-PASSWORD-DO-NOT-USE"));
    assert!(result
        .report
        .iter()
        .any(|i| i.level == ReportLevel::Warning && i.message.contains("will not mark")));
}

#[test]
fn url_and_form_credentials_are_lifted_into_secret_variables() {
    let dir = tempfile::tempdir().unwrap();
    let doc = json!({"info": {"name": "Urls", "schema": SCHEMA}, "item": [
        {"name": "u", "request": {"method": "POST", "url": "https://bob:PW-IN-URL@x.test/a?api_key=KEY-IN-QUERY&q=1",
            "body": {"mode": "urlencoded", "urlencoded": [{"key": "password", "value": "PW-IN-FORM"}, {"key": "name", "value": "ok"}]}}}]});
    let path = temp_json(&dir, "u.postman_collection.json", &doc);
    let mut world = World::new();

    let result = world.import_paths(&[path], &import_options(true)).unwrap();

    let id = result.collection_id.unwrap();
    let files = snapshot(&world.root(&id));
    for leaked in ["PW-IN-URL", "KEY-IN-QUERY", "PW-IN-FORM"] {
        assert!(!any_contains(&files, leaked), "{leaked}");
        assert!(!format!("{:?}", result.report).contains(leaked));
    }
    assert!(any_contains(&files, "q=1") && any_contains(&files, "{{u_url_password}}"));
    let vars = world.store.collection_variables(&id).unwrap();
    assert!(vars.values().filter(|v| v.secret).count() >= 3);
    assert_eq!(
        world
            .vault
            .get(&format!("c/{id}/u_url_password"))
            .unwrap()
            .as_deref(),
        Some("PW-IN-URL")
    );
}
