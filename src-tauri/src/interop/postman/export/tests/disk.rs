//! Exports a real collection on disk and validates every output with the vendored
//! draft-04 v2.1.0 schema (plan 09 §2, D10).

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::super::*;
use crate::interop::postman::schema::{parse_collection, Item};
use crate::store::collection::Collection;
use crate::store::env_model::{VarEntry, VarTable};
use crate::store::types::{
    Auth, Body, KeyValue, NodeKind, RequestContent, RequestKind, Scripts, TreeNode,
};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/postman")
}

fn assert_valid(json: &str) {
    let schema_text = std::fs::read_to_string(fixtures().join("schema/collection-v2.1.0.json"))
        .expect("vendored schema");
    let schema: Value = serde_json::from_str(&schema_text).expect("schema json");
    let validator = jsonschema::validator_for(&schema).expect("schema compiles");
    let instance: Value = serde_json::from_str(json).expect("export json");
    let errors: Vec<String> = validator
        .iter_errors(&instance)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "schema errors: {errors:#?}");
}

fn sample(dir: &Path) -> Collection {
    let c = Collection::create(dir, "Demo API").expect("create");
    let folder = c.create_folder("", "Users").expect("folder");
    c.write_folder_scripts(
        &folder,
        &Scripts {
            enabled: true,
            pre_request: "pm.test('f');".into(),
            post_response: String::new(),
        },
    )
    .expect("folder scripts");
    let get = c
        .create_request(&folder, "List users", RequestKind::Http)
        .expect("request");
    c.write_request(
        &get,
        &RequestContent {
            headers: vec![KeyValue {
                name: "Accept".into(),
                value: "application/json".into(),
                enabled: true,
            }],
            auth: Auth::Bearer {
                token: "{{token}}".into(),
            },
            scripts: Scripts {
                enabled: true,
                pre_request: String::new(),
                post_response: "pm.test('ok');\n".into(),
            },
            ..RequestContent::new("GET", "{{base}}/users?page=1")
        },
    )
    .expect("write get");
    let post = c
        .create_request("", "Create", RequestKind::Http)
        .expect("request");
    c.write_request(
        &post,
        &RequestContent {
            body: Body::Json {
                text: "{\"a\":1}".into(),
            },
            ..RequestContent::new("POST", "https://api.io/items")
        },
    )
    .expect("write post");
    let gql = c
        .create_request("", "Query", RequestKind::Graphql)
        .expect("gql");
    let mut gql_content = c
        .read_request(&gql)
        .expect("read")
        .content()
        .expect("content");
    gql_content.url = "https://api.io/graphql".into();
    c.write_request(&gql, &gql_content).expect("write gql");
    c.create_request("", "Socket", RequestKind::Ws).expect("ws");
    c.create_request("", "Rpc", RequestKind::Grpc)
        .expect("grpc");
    c.write_collection_variables(VarTable::from([
        (
            "base".to_string(),
            VarEntry::new("https://api.io".into(), true, false),
        ),
        (
            "token".to_string(),
            VarEntry::new(String::new(), true, true),
        ),
    ]))
    .expect("vars");
    c
}

fn names(nodes: &[TreeNode]) -> Vec<String> {
    nodes
        .iter()
        .filter(|n| n.kind != NodeKind::Invalid)
        .map(|n| n.name.clone())
        .collect()
}

#[test]
fn exported_collection_is_schema_valid_deterministic_and_ordered() {
    let dir = tempfile::tempdir().expect("tmp");
    let c = sample(dir.path());
    let first =
        export_collection(&c, "11111111-1111-4111-8111-111111111111", None).expect("export");
    let second =
        export_collection(&c, "11111111-1111-4111-8111-111111111111", None).expect("export");
    assert_eq!(first, second);
    assert_eq!(first.file_name, "demo-api.postman_collection.json");
    assert_valid(&first.json);

    let parsed =
        parse_collection(&serde_json::from_str(&first.json).expect("json")).expect("parse");
    let pm = parsed.value;
    assert_eq!(parsed.unknown_fields, 0);
    assert_eq!(pm.info.name, "Demo API");
    assert_eq!(pm.info.schema, crate::interop::postman::schema::SCHEMA_V21);
    let exported: Vec<String> = pm
        .item
        .iter()
        .map(|i| match i {
            Item::Folder(f) => f.name.clone(),
            Item::Request(r) => r.name.clone(),
        })
        .collect();
    let tree: Vec<String> = names(&c.tree())
        .into_iter()
        .filter(|n| n != "Socket" && n != "Rpc")
        .collect();
    assert_eq!(exported, tree);
    let Some(Item::Folder(users)) = pm.item.iter().find(|i| matches!(i, Item::Folder(_))) else {
        panic!("folder exported");
    };
    assert_eq!(users.event[0].listen, "prerequest");
    assert_eq!(pm.variable[1].kind.as_deref(), Some("string"));
    assert!(first.report.iter().any(|r| r.path == "variable.token"));
    assert_eq!(
        first
            .report
            .iter()
            .filter(|r| r.level == ReportLevel::Skipped)
            .count(),
        2
    );
}

#[test]
fn reuses_imported_postman_id_and_includes_secrets_only_when_passed() {
    let dir = tempfile::tempdir().expect("tmp");
    let c = sample(dir.path());
    let path = c.root.join("papan.collection.yaml");
    let yaml = std::fs::read_to_string(&path).expect("read");
    std::fs::write(&path, format!("postman_id: pm-42\n{yaml}")).expect("write");
    let plain = export_collection(&c, "fallback", None).expect("export");
    assert!(plain.json.contains("\"_postman_id\": \"pm-42\""));
    assert!(!plain.json.contains("fallback"));
    assert!(!plain.json.contains("s3cr3t"));
    let secrets = SecretValues::from([("token".to_string(), "s3cr3t".to_string())]);
    let with = export_collection(&c, "fallback", Some(&secrets)).expect("export");
    assert!(with.json.contains("s3cr3t"));
    assert_valid(&with.json);
}

#[test]
fn every_vendored_fixture_reads_and_writes_back_schema_valid() {
    let mut seen = 0;
    for dir in ["v21", "v20"] {
        let Ok(entries) = std::fs::read_dir(fixtures().join(dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            let text = std::fs::read_to_string(entry.path()).expect("fixture");
            let value: Value = serde_json::from_str(&text).expect("fixture json");
            let parsed = parse_collection(&value)
                .unwrap_or_else(|e| panic!("{}: {e}", entry.path().display()));
            let mut pm = parsed.value;
            pm.info.schema = crate::interop::postman::schema::SCHEMA_V21.into();
            assert_valid(&to_json(&pm).expect("json"));
            seen += 1;
        }
    }
    assert!(seen >= 7, "fixtures found: {seen}");
}
