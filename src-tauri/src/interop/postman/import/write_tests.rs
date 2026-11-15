use std::path::PathBuf;
use std::sync::Arc;

use super::super::tree::{PlannedFolder, PlannedNode};
use super::super::vault::apply_secrets;
use super::super::{plan_files, with_options};
use super::*;
use crate::interop::postman::limits::file_label;
use crate::interop::postman::types::ImportTarget;
use crate::store::env_model::VarEntry;
use crate::store::model::{FolderFile, RequestFile};
use crate::store::types::RequestKind;
use crate::vars::vault::{MemoryVault, SecretStore};
use crate::vars::VarsService;

const FAKE_ENV_KEY: &str = "FAKE-ENV-API-KEY-DO-NOT-USE";

fn fixture(rel: &str) -> (String, Vec<u8>) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/postman")
        .join(rel);
    (file_label(&path), std::fs::read(path).unwrap())
}

fn options(store_secret_values: bool, clash: GlobalsClash) -> ImportOptions {
    ImportOptions {
        target: ImportTarget::NewCollection,
        secret_names: Vec::new(),
        store_secret_values,
        import_scripts: true,
        globals_clash: clash,
    }
}

fn full_plan() -> ImportPlan {
    plan_files(&[
        fixture("v21/auth.postman_collection.json"),
        fixture("env/dev.postman_environment.json"),
        fixture("env/globals.postman_globals.json"),
    ])
    .unwrap()
    .0
}

/// Every byte under `dir`, concatenated (to look for leaked secrets).
fn all_bytes(dir: &Path) -> Vec<u8> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(all_bytes(&path));
        } else {
            out.extend(std::fs::read(&path).unwrap());
        }
    }
    out
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[test]
fn new_collection_is_written_opened_and_holds_no_secret_value() {
    let app = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    let store = Store::new(app.path()).unwrap();
    let plan = full_plan();

    let written = write_new_collection(
        &plan,
        &options(false, GlobalsClash::Keep),
        parent.path(),
        &store,
    )
    .unwrap();

    let opened = Collection::open(&written.root).unwrap();
    assert_eq!(opened.name, "Auth v2.1");
    assert_eq!(written.environments, ["dev"]);
    assert!(opened.read_env("dev").unwrap().variables["apiKey"].secret);
    assert!(!opened.tree().is_empty());
    assert_eq!(
        names_in(parent.path()).unwrap(),
        [file_label(&written.root)]
    );
    assert!(!contains(&all_bytes(&written.root), FAKE_ENV_KEY));
    assert_eq!(written.pending, PendingSecrets::default());
    assert!(store.globals().unwrap().variables.contains_key("userAgent"));
    assert!(written.globals > 0);
}

#[test]
fn a_second_import_of_the_same_name_gets_a_new_folder() {
    let app = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    let store = Store::new(app.path()).unwrap();
    let plan = full_plan();
    let opts = options(false, GlobalsClash::Keep);

    let first = write_new_collection(&plan, &opts, parent.path(), &store).unwrap();
    let second = write_new_collection(&plan, &opts, parent.path(), &store).unwrap();

    assert_ne!(first.root, second.root);
    assert!(Collection::open(&first.root).is_ok());
}

fn broken_plan() -> ImportPlan {
    let request = RequestFile::new("R".into(), "GET", RequestKind::Http);
    let node = PlannedNode::Request {
        entry: "r.http.yaml".into(),
        file: request,
    };
    let mut plan = full_plan();
    let collection = plan.collection.as_mut().unwrap();
    collection.root = PlannedFolder {
        dir: String::new(),
        file: FolderFile::default(),
        children: vec![node.clone(), node],
    };
    plan
}

#[test]
fn a_failure_while_writing_leaves_nothing_behind() {
    let app = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    std::fs::write(parent.path().join("keep.txt"), "x").unwrap();
    let store = Store::new(app.path()).unwrap();

    let result = write_new_collection(
        &broken_plan(),
        &options(false, GlobalsClash::Keep),
        parent.path(),
        &store,
    );

    assert!(result.is_err());
    assert_eq!(names_in(parent.path()).unwrap(), ["keep.txt"]);
    assert!(
        store.globals().unwrap().variables.is_empty(),
        "globals written after a failure"
    );
}

#[test]
fn globals_clash_keep_and_overwrite() {
    let app = tempfile::tempdir().unwrap();
    let store = Store::new(app.path()).unwrap();
    let existing: VarTable = [(
        "userAgent".to_string(),
        VarEntry::new("mine".into(), true, false),
    )]
    .into();
    store.write_globals(existing).unwrap();
    let plan = full_plan();

    let parent = tempfile::tempdir().unwrap();
    write_new_collection(
        &plan,
        &options(false, GlobalsClash::Keep),
        parent.path(),
        &store,
    )
    .unwrap();
    assert_eq!(
        store.globals().unwrap().variables["userAgent"].value,
        "mine"
    );

    write_new_collection(
        &plan,
        &options(false, GlobalsClash::Overwrite),
        parent.path(),
        &store,
    )
    .unwrap();
    assert_eq!(
        store.globals().unwrap().variables["userAgent"].value,
        "papan-fixture"
    );
}

#[test]
fn opted_in_secret_values_reach_the_vault_only_through_apply_secrets() {
    let app = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    let mut store = Store::new(app.path()).unwrap();
    let vault = Arc::new(MemoryVault::default());
    let vars = VarsService::new(vault.clone());
    let plan = full_plan();

    let written = write_new_collection(
        &plan,
        &options(true, GlobalsClash::Keep),
        parent.path(),
        &store,
    )
    .unwrap();
    let id = store.open_collection(&written.root).unwrap().collection.id;
    let report = apply_secrets(&vars, &mut store, &id, &written.pending);

    assert!(report.items().is_empty(), "{:?}", report.items());
    let stored = vault.get(&format!("e/{id}/dev/apiKey")).unwrap();
    assert_eq!(stored.as_deref(), Some(FAKE_ENV_KEY));
    assert!(vault.get("g/globalApiKey").unwrap().is_some());
    assert!(!format!("{:?}", written.pending).contains(FAKE_ENV_KEY));
}

#[test]
fn import_into_an_open_collection_adds_a_folder_envs_and_free_variables() {
    let app = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    let mut store = Store::new(app.path()).unwrap();
    let id = store
        .create_collection(parent.path(), "Mine")
        .unwrap()
        .collection
        .id;
    let kept: VarTable = [(
        "token".to_string(),
        VarEntry::new("own".into(), true, false),
    )]
    .into();
    store.write_collection_variables(&id, kept).unwrap();
    let plan = with_options(&full_plan(), &options(false, GlobalsClash::Keep));

    let written =
        write_into_collection(&plan, &options(false, GlobalsClash::Keep), &mut store, &id).unwrap();
    let again =
        write_into_collection(&plan, &options(false, GlobalsClash::Keep), &mut store, &id).unwrap();

    let collection = store.collection(&id).unwrap();
    assert_eq!(written.folder.as_deref(), Some("auth-v2-1"));
    assert_eq!(again.folder.as_deref(), Some("auth-v2-1-2"));
    assert_eq!(again.environments, ["dev-2"]);
    assert_eq!(collection.env_stems(), ["dev", "dev-2"]);
    let tree = collection.tree();
    assert_eq!(tree.len(), 2);
    assert_eq!(tree[0].name, "Auth v2.1");
    assert_eq!(
        store.collection_variables(&id).unwrap()["token"].value,
        "own"
    );
    let leftovers: Vec<String> = names_in(&collection.root)
        .unwrap()
        .into_iter()
        .chain(names_in(&collection.requests_root()).unwrap())
        .filter(|name| name.starts_with(STAGING_PREFIX))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn a_failed_import_into_a_collection_leaves_it_unchanged() {
    let app = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    let mut store = Store::new(app.path()).unwrap();
    let id = store
        .create_collection(parent.path(), "Mine")
        .unwrap()
        .collection
        .id;
    let root = store.collection(&id).unwrap().root;
    let before = all_bytes(&root);

    let result = write_into_collection(
        &broken_plan(),
        &options(false, GlobalsClash::Keep),
        &mut store,
        &id,
    );

    assert!(result.is_err());
    assert_eq!(all_bytes(&root), before);
    assert!(names_in(&root.join("requests")).unwrap().is_empty());
}

#[test]
fn environment_only_import_needs_no_collection_in_the_plan() {
    let app = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    let mut store = Store::new(app.path()).unwrap();
    let id = store
        .create_collection(parent.path(), "Mine")
        .unwrap()
        .collection
        .id;
    let (plan, _) = plan_files(&[fixture("env/dev.postman_environment.json")]).unwrap();

    let written =
        write_into_collection(&plan, &options(false, GlobalsClash::Keep), &mut store, &id).unwrap();
    let refused = write_new_collection(
        &plan,
        &options(false, GlobalsClash::Keep),
        parent.path(),
        &store,
    );

    assert_eq!(written.folder, None);
    assert_eq!(written.environments, ["dev"]);
    assert!(refused.is_err());
}
