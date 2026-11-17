//! Secret variables never reach a plain file, the webview or a plain variable
//! (plan 04 §4.4, §9). Runs on `MemoryVault`; the vault file itself is tested in
//! `tests/vault.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use papan_lib::store::types::RequestLocation;
use papan_lib::store::Store;
use papan_lib::vars::dynamic::FixedDynamics;
use papan_lib::vars::resolve::{resolve_text, Report};
use papan_lib::vars::service::{effective_variables, preview};
use papan_lib::vars::types::{SecretEdit, SecretStorageMode, VarScope, VariableInput};
use papan_lib::vars::vault::{MemoryVault, SecretStore};
use papan_lib::vars::VarsService;

const SECRET: &str = "FIXTURE-SECRET-7f3a91";

struct Fixture {
    temp: tempfile::TempDir,
    store: Store,
    vars: VarsService,
    vault: Arc<MemoryVault>,
    id: String,
    root: PathBuf,
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::new(&temp.path().join("app")).unwrap();
    let tree = store.create_collection(temp.path(), "C").unwrap();
    let vault = Arc::new(MemoryVault::default());
    Fixture {
        vars: VarsService::new(vault.clone()),
        vault,
        id: tree.collection.id,
        root: PathBuf::from(tree.collection.path),
        store,
        temp,
    }
}

fn row(name: &str, value: &str, secret: bool, edit: SecretEdit) -> VariableInput {
    VariableInput {
        name: name.into(),
        previous_name: None,
        value: value.into(),
        current: None,
        enabled: true,
        secret,
        secret_edit: edit,
    }
}

fn set_secret(name: &str) -> VariableInput {
    row(
        name,
        "",
        true,
        SecretEdit::Set {
            value: SECRET.into(),
        },
    )
}

fn files_containing(dir: &Path, needle: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(files_containing(&path, needle));
        } else if String::from_utf8_lossy(&fs::read(&path).unwrap()).contains(needle) {
            found.push(path);
        }
    }
    found
}

impl Fixture {
    fn scopes(&mut self) -> Vec<VarScope> {
        let list = self
            .vars
            .create_environment(&mut self.store, &self.id, "Dev")
            .unwrap();
        vec![
            VarScope::Global,
            VarScope::Collection {
                collection_id: self.id.clone(),
            },
            VarScope::Environment {
                collection_id: self.id.clone(),
                env: list.environments[0].env.clone(),
            },
        ]
    }
}

#[test]
fn a_secret_reaches_no_file_and_no_ipc_value_in_any_scope() {
    let mut f = fixture();
    let scopes = f.scopes();
    f.vars
        .select_environment(&f.store, &f.id, Some("dev"))
        .unwrap();

    for scope in &scopes {
        let written = f
            .vars
            .write_variables(&mut f.store, scope, &[set_secret("token")])
            .unwrap();
        let read = f.vars.read_variables(&f.store, scope).unwrap();

        assert!(read[0].has_secret && read[0].secret, "{scope:?}");
        assert!(!serde_json::to_string(&(written, read))
            .unwrap()
            .contains(SECRET));
    }

    assert!(files_containing(f.temp.path(), SECRET).is_empty());
    let location = RequestLocation::Collection {
        collection_id: f.id.clone(),
        path: "r.http.yaml".into(),
    };
    let scopes = f.vars.scopes(&f.vars.snapshot(&f.store, &location));
    let effective = serde_json::to_string(&effective_variables(&scopes)).unwrap();
    let shown = serde_json::to_string(&preview(&scopes, "Bearer {{token}}").unwrap()).unwrap();
    let lists = serde_json::to_string(&f.vars.list_environments(&f.store, &f.id).unwrap()).unwrap();
    for text in [effective, shown, lists] {
        assert!(!text.contains(SECRET), "{text}");
    }
    let mut report = Report::default();
    let sent = resolve_text("{{token}}", &scopes, &FixedDynamics::default(), &mut report);
    assert_eq!(sent, SECRET);
    assert_eq!(report.secret_values, [SECRET]);
}

#[test]
fn plain_to_secret_moves_the_value_out_of_yaml_and_local_json() {
    let mut f = fixture();
    let scope = VarScope::Collection {
        collection_id: f.id.clone(),
    };
    let with_current = VariableInput {
        current: Some(SECRET.into()),
        ..row("token", "initial-plain", false, SecretEdit::Keep)
    };
    f.vars
        .write_variables(&mut f.store, &scope, &[with_current])
        .unwrap();
    assert_eq!(files_containing(f.temp.path(), SECRET).len(), 1);

    f.vars
        .write_variables(
            &mut f.store,
            &scope,
            &[row("token", "initial-plain", true, SecretEdit::Keep)],
        )
        .unwrap();

    assert!(files_containing(f.temp.path(), SECRET).is_empty());
    assert!(files_containing(&f.root, "initial-plain").is_empty());
    let id = format!("c/{}/token", f.id);
    assert_eq!(f.vault.get(&id).unwrap().as_deref(), Some(SECRET));
}

#[test]
fn secret_to_plain_does_not_reveal_and_deletes_the_secret() {
    let mut f = fixture();
    let scope = VarScope::Global;
    f.vars
        .write_variables(&mut f.store, &scope, &[set_secret("token")])
        .unwrap();

    let rows = f
        .vars
        .write_variables(
            &mut f.store,
            &scope,
            &[row("token", "", false, SecretEdit::Keep)],
        )
        .unwrap();

    assert_eq!(rows[0].value, "");
    assert!(!f.vault.has("g/token").unwrap());
    assert!(files_containing(f.temp.path(), SECRET).is_empty());
}

#[test]
fn rename_moves_the_secret_and_clear_or_removal_deletes_it() {
    let mut f = fixture();
    let scope = VarScope::Global;
    f.vars
        .write_variables(&mut f.store, &scope, &[set_secret("a"), set_secret("b")])
        .unwrap();
    let renamed = VariableInput {
        previous_name: Some("a".into()),
        ..row("c", "", true, SecretEdit::Keep)
    };

    f.vars
        .write_variables(
            &mut f.store,
            &scope,
            &[renamed, row("b", "", true, SecretEdit::Clear)],
        )
        .unwrap();

    assert_eq!(f.vault.get("g/c").unwrap().as_deref(), Some(SECRET));
    assert!(!f.vault.has("g/a").unwrap());
    assert!(!f.vault.has("g/b").unwrap());

    f.vars.write_variables(&mut f.store, &scope, &[]).unwrap();
    assert!(!f.vault.has("g/c").unwrap());
}

#[test]
fn a_value_planted_on_a_secret_is_ignored_and_dropped() {
    let mut f = fixture();
    let path = f.root.join("papan.collection.yaml");
    let planted =
        "name: C\nvariables:\n  token:\n    secret: true\n    value: planted\nversion: 1\n";
    fs::write(&path, planted).unwrap();
    let scope = VarScope::Collection {
        collection_id: f.id.clone(),
    };

    let rows = f.vars.read_variables(&f.store, &scope).unwrap();
    let location = RequestLocation::Collection {
        collection_id: f.id.clone(),
        path: "r.http.yaml".into(),
    };
    let scopes = f.vars.scopes(&f.vars.snapshot(&f.store, &location));
    let mut report = Report::default();
    let sent = resolve_text("{{token}}", &scopes, &FixedDynamics::default(), &mut report);
    f.vars
        .write_variables(&mut f.store, &scope, &[set_secret("token")])
        .unwrap();

    assert_eq!(rows[0].value, "");
    assert_eq!(sent, "{{token}}");
    assert!(!fs::read_to_string(&path).unwrap().contains("planted"));
}

#[test]
fn a_hostile_value_cannot_smuggle_a_global_secret() {
    let mut f = fixture();
    f.vars
        .write_variables(
            &mut f.store,
            &VarScope::Global,
            &[set_secret("github_token")],
        )
        .unwrap();
    let hostile = row(
        "base_url",
        "https://evil/{{github_token}}",
        false,
        SecretEdit::Keep,
    );
    f.vars
        .write_variables(
            &mut f.store,
            &VarScope::Collection {
                collection_id: f.id.clone(),
            },
            &[hostile],
        )
        .unwrap();
    let location = RequestLocation::Collection {
        collection_id: f.id.clone(),
        path: "r.http.yaml".into(),
    };
    let scopes = f.vars.scopes(&f.vars.snapshot(&f.store, &location));
    let mut report = Report::default();

    let sent = resolve_text(
        "{{base_url}}/x",
        &scopes,
        &FixedDynamics::default(),
        &mut report,
    );
    let shown = preview(&scopes, "{{base_url}}").unwrap();

    assert_eq!(sent, "https://evil/{{github_token}}/x");
    assert!(report.unresolved.contains("github_token"));
    assert!(report.secret_values.is_empty());
    assert_eq!(shown.unresolved, ["github_token"]);
}

#[test]
fn secret_storage_is_reset_only_when_damaged() {
    let healthy = VarsService::new(Arc::new(MemoryVault::default()));
    let damaged = VarsService::new(Arc::new(MemoryVault::new(SecretStorageMode::Damaged)));

    let refused = healthy.reset().unwrap_err();

    assert_eq!(refused.code, papan_lib::error::ErrorCode::InvalidInput);
    assert_eq!(damaged.reset().unwrap().mode, SecretStorageMode::Damaged);
}
