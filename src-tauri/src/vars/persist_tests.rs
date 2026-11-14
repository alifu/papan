use std::sync::Arc;

use super::*;
use crate::vars::types::{SecretEdit, VariableInput};
use crate::vars::vault::{MemoryVault, SecretStore};

struct Fixture {
    _dir: tempfile::TempDir,
    store: Store,
    vars: VarsService,
    vault: Arc<MemoryVault>,
    collection: VarScope,
    env: VarScope,
}

fn input(name: &str, value: &str, secret: bool) -> VariableInput {
    VariableInput {
        name: name.into(),
        previous_name: None,
        value: if secret { String::new() } else { value.into() },
        current: None,
        enabled: true,
        secret,
        secret_edit: if secret {
            SecretEdit::Set {
                value: value.into(),
            }
        } else {
            SecretEdit::Keep
        },
    }
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(&dir.path().join("data")).unwrap();
    let id = store
        .create_collection(dir.path(), "C")
        .unwrap()
        .collection
        .id;
    let vault = Arc::new(MemoryVault::default());
    let vars = VarsService::new(vault.clone());
    let list = vars.create_environment(&mut store, &id, "Dev").unwrap();
    let env = VarScope::Environment {
        collection_id: id.clone(),
        env: list.environments[0].env.clone(),
    };
    vars.write_variables(
        &mut store,
        &env,
        &[input("host", "shared", false), input("key", "s3cr3t", true)],
    )
    .unwrap();
    Fixture {
        _dir: dir,
        store,
        vars,
        vault,
        collection: VarScope::Collection { collection_id: id },
        env,
    }
}

fn write(scope: &VarScope, name: &str, value: Option<&str>) -> ScriptWrite {
    ScriptWrite {
        scope: scope.clone(),
        name: name.into(),
        value: value.map(str::to_string),
    }
}

fn find(f: &Fixture, scope: &VarScope, name: &str) -> Option<crate::vars::types::Variable> {
    let vars = f.vars.read_variables(&f.store, scope).unwrap();
    vars.into_iter().find(|v| v.name == name)
}

#[test]
fn a_set_on_an_existing_variable_changes_only_its_current_value() {
    let mut f = fixture();
    let env = f.env.clone();

    let refused =
        f.vars
            .apply_script_writes(&mut f.store, &[write(&env, "host", Some("local"))], &[]);

    assert!(refused.is_empty());
    let host = find(&f, &env, "host").unwrap();
    assert_eq!(host.value, "shared");
    assert_eq!(host.current.as_deref(), Some("local"));
}

#[test]
fn a_new_variable_is_created_with_an_empty_shared_value() {
    let mut f = fixture();
    let scope = f.collection.clone();

    f.vars
        .apply_script_writes(&mut f.store, &[write(&scope, "token", Some("abc"))], &[]);

    let token = find(&f, &scope, "token").unwrap();
    assert_eq!(token.value, "");
    assert_eq!(token.current.as_deref(), Some("abc"));
    assert!(!token.secret);
}

#[test]
fn a_secret_variable_goes_to_the_vault_and_an_unset_deletes_it() {
    let mut f = fixture();
    let env = f.env.clone();
    let VarScope::Environment {
        collection_id,
        env: name,
    } = &env
    else {
        unreachable!()
    };
    let id = format!("e/{collection_id}/{name}/key");

    f.vars
        .apply_script_writes(&mut f.store, &[write(&env, "key", Some("rotated"))], &[]);
    assert_eq!(f.vault.get(&id).unwrap().as_deref(), Some("rotated"));
    assert_eq!(find(&f, &env, "key").unwrap().current, None);

    f.vars
        .apply_script_writes(&mut f.store, &[write(&env, "key", None)], &[]);
    assert_eq!(f.vault.get(&id).unwrap(), None);
}

#[test]
fn globals_are_written_to_the_globals_file_and_unset_removes_them() {
    let mut f = fixture();

    f.vars.apply_script_writes(
        &mut f.store,
        &[write(&VarScope::Global, "g", Some("1"))],
        &[],
    );
    assert_eq!(find(&f, &VarScope::Global, "g").unwrap().value, "1");

    f.vars
        .apply_script_writes(&mut f.store, &[write(&VarScope::Global, "g", None)], &[]);
    assert_eq!(find(&f, &VarScope::Global, "g"), None);
}

#[test]
fn an_unset_removes_a_script_created_variable_but_keeps_a_shared_one() {
    let mut f = fixture();
    let env = f.env.clone();
    f.vars.apply_script_writes(
        &mut f.store,
        &[
            write(&env, "tmp", Some("x")),
            write(&env, "host", Some("y")),
        ],
        &[],
    );

    f.vars.apply_script_writes(
        &mut f.store,
        &[write(&env, "tmp", None), write(&env, "host", None)],
        &[],
    );

    assert_eq!(find(&f, &env, "tmp"), None);
    let host = find(&f, &env, "host").unwrap();
    assert_eq!((host.value.as_str(), host.current), ("shared", None));
}

#[test]
fn an_invalid_name_is_refused_without_blocking_the_other_writes() {
    let mut f = fixture();
    let scope = f.collection.clone();

    let refused = f.vars.apply_script_writes(
        &mut f.store,
        &[
            write(&scope, "  ", Some("x")),
            write(&scope, "ok", Some("1")),
        ],
        &[],
    );

    assert_eq!(refused.len(), 1);
    assert_eq!(
        find(&f, &scope, "ok").unwrap().current.as_deref(),
        Some("1")
    );
}

#[test]
fn a_known_secret_value_is_not_written_to_a_plain_variable() {
    let mut f = fixture();
    let (env, collection) = (f.env.clone(), f.collection.clone());
    let secrets = ["s3cr3t".to_string()];

    let refused = f.vars.apply_script_writes(
        &mut f.store,
        &[
            write(&env, "host", Some("s3cr3t")),
            write(&collection, "copy", Some("s3cr3t")),
            write(&VarScope::Global, "g", Some("s3cr3t")),
            write(&env, "key", Some("s3cr3t")),
            write(&env, "other", Some("plain")),
        ],
        &secrets,
    );

    assert_eq!(refused.len(), 3, "{refused:?}");
    assert!(refused.iter().all(|line| !line.contains("s3cr3t")));
    assert_eq!(find(&f, &env, "host").unwrap().current, None);
    assert_eq!(find(&f, &collection, "copy"), None);
    assert_eq!(find(&f, &VarScope::Global, "g"), None);
    assert_eq!(
        find(&f, &env, "other").unwrap().current.as_deref(),
        Some("plain")
    );
}

#[test]
fn a_set_on_a_disabled_variable_enables_it() {
    let mut f = fixture();
    let env = f.env.clone();
    let off = VariableInput {
        enabled: false,
        ..input("off", "shared", false)
    };
    let inputs = [input("host", "shared", false), input("key", "", true), off];
    let kept = inputs.map(|row| VariableInput {
        secret_edit: SecretEdit::Keep,
        ..row
    });
    f.vars.write_variables(&mut f.store, &env, &kept).unwrap();
    assert!(!find(&f, &env, "off").unwrap().enabled);

    let refused = f.vars.apply_script_writes(
        &mut f.store,
        &[write(&env, "off", Some("now")), write(&env, "host", None)],
        &[],
    );

    assert!(refused.is_empty());
    let off = find(&f, &env, "off").unwrap();
    assert_eq!((off.enabled, off.current.as_deref()), (true, Some("now")));
}
