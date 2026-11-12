//! Environments and variables on disk through `VarsService` (plan 04 §3, §7, §9).

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use papan_lib::error::ErrorCode;
use papan_lib::store::types::RequestLocation;
use papan_lib::store::{watcher, Store};
use papan_lib::vars::dynamic::FixedDynamics;
use papan_lib::vars::resolve::{resolve_text, Report};
use papan_lib::vars::service::{effective_variables, preview};
use papan_lib::vars::types::{SecretEdit, VarScope, VarSource, Variable, VariableInput};
use papan_lib::vars::vault::{MemoryVault, SecretStore};
use papan_lib::vars::VarsService;

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

fn plain(name: &str, value: &str) -> VariableInput {
    VariableInput {
        name: name.into(),
        previous_name: None,
        value: value.into(),
        current: None,
        enabled: true,
        secret: false,
        secret_edit: SecretEdit::Keep,
    }
}

impl Fixture {
    fn env_scope(&self, env: &str) -> VarScope {
        VarScope::Environment {
            collection_id: self.id.clone(),
            env: env.into(),
        }
    }

    fn collection_scope(&self) -> VarScope {
        VarScope::Collection {
            collection_id: self.id.clone(),
        }
    }

    fn location(&self) -> RequestLocation {
        RequestLocation::Collection {
            collection_id: self.id.clone(),
            path: "r.http.yaml".into(),
        }
    }

    fn create(&mut self, name: &str) -> String {
        let list = self
            .vars
            .create_environment(&mut self.store, &self.id, name)
            .unwrap();
        list.environments
            .iter()
            .find(|e| e.name == name)
            .unwrap()
            .env
            .clone()
    }

    fn write(&mut self, scope: &VarScope, rows: &[VariableInput]) -> Vec<Variable> {
        self.vars
            .write_variables(&mut self.store, scope, rows)
            .unwrap()
    }

    fn resolve(&self, text: &str) -> (String, Report) {
        let snapshot = self.vars.snapshot(&self.store, &self.location());
        let scopes = self.vars.scopes(&snapshot);
        let mut report = Report::default();
        let out = resolve_text(text, &scopes, &FixedDynamics::default(), &mut report);
        (out, report)
    }
}

#[test]
fn create_rename_and_delete_environments() {
    let mut f = fixture();
    let dev = f.create("Dev");
    let dev2 = f.create("dev");
    f.write(&f.env_scope(&dev), &[plain("a", "1")]);
    f.vault
        .apply(&[papan_lib::vars::vault::SecretChange::Set {
            id: format!("e/{}/{dev}/s", f.id),
            value: "x".into(),
        }])
        .unwrap();

    let renamed = f
        .vars
        .rename_environment(&mut f.store, &f.id, &dev, "Zeta")
        .unwrap();
    assert_eq!((dev.as_str(), dev2.as_str()), ("dev", "dev-2"));
    assert!(renamed
        .environments
        .iter()
        .any(|e| e.env == dev && e.name == "Zeta"));
    assert!(f.root.join("environments/dev.env.yaml").is_file());

    f.vars
        .select_environment(&f.store, &f.id, Some(&dev))
        .unwrap();
    let after = f
        .vars
        .delete_environment(&mut f.store, &f.id, &dev)
        .unwrap();

    assert_eq!(after.environments.len(), 1);
    assert_eq!(after.selected, None);
    assert!(!f.vault.has(&format!("e/{}/{dev}/s", f.id)).unwrap());
    assert!(!f.root.join("environments/dev.env.yaml").exists());
}

#[test]
fn environments_sort_case_insensitively_then_by_stem() {
    let mut f = fixture();
    for name in ["beta", "Alpha", "alpha", "Gamma"] {
        f.create(name);
    }

    let list = f.vars.list_environments(&f.store, &f.id).unwrap();

    let order: Vec<(&str, &str)> = list
        .environments
        .iter()
        .map(|e| (e.name.as_str(), e.env.as_str()))
        .collect();
    assert_eq!(
        order,
        [
            ("Alpha", "alpha"),
            ("alpha", "alpha-2"),
            ("beta", "beta"),
            ("Gamma", "gamma")
        ]
    );
}

#[test]
fn hostile_env_values_are_refused_and_touch_nothing_outside() {
    let mut f = fixture();
    let outside = f.temp.path().join("x.env.yaml");
    fs::write(&outside, "name: Outside\n").unwrap();

    for env in ["../x", "a/b", "a.b", "/etc/passwd", "", ".hidden"] {
        let error = f
            .vars
            .rename_environment(&mut f.store, &f.id, env, "N")
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{env:?}");
        let read = f
            .vars
            .read_variables(&f.store, &f.env_scope(env))
            .unwrap_err();
        assert_eq!(read.code, ErrorCode::InvalidInput, "{env:?}");
    }
    assert_eq!(fs::read_to_string(&outside).unwrap(), "name: Outside\n");
}

#[test]
fn an_env_stem_must_match_the_file_name_exactly() {
    let mut f = fixture();
    let dev = f.create("Dev");
    f.write(&f.env_scope(&dev), &[plain("a", "1")]);

    for other_case in ["DEV", "Dev", "dEv"] {
        let error = f
            .vars
            .read_variables(&f.store, &f.env_scope(other_case))
            .unwrap_err();
        assert!(
            matches!(error.code, ErrorCode::InvalidInput | ErrorCode::NotFound),
            "{other_case}"
        );
        let delete = f.vars.delete_environment(&mut f.store, &f.id, other_case);
        assert!(delete.is_err(), "{other_case}");
    }
    assert!(f.root.join("environments/dev.env.yaml").is_file());
}

#[cfg(unix)]
#[test]
fn symlinked_env_files_are_ignored() {
    let mut f = fixture();
    f.create("Dev");
    let target = f.temp.path().join("real.env.yaml");
    fs::write(&target, "name: Linked\n").unwrap();
    std::os::unix::fs::symlink(&target, f.root.join("environments/linked.env.yaml")).unwrap();

    let list = f.vars.list_environments(&f.store, &f.id).unwrap();

    assert_eq!(list.environments.len(), 1);
    assert!(f
        .vars
        .read_variables(&f.store, &f.env_scope("linked"))
        .is_err());
}

#[test]
fn at_most_fifty_environments_and_invalid_files_are_flagged() {
    let mut f = fixture();
    let dir = f.root.join("environments");
    fs::create_dir_all(&dir).unwrap();
    for n in 0..51 {
        fs::write(
            dir.join(format!("e{n:02}.env.yaml")),
            format!("name: E{n:02}\n"),
        )
        .unwrap();
    }
    fs::write(dir.join("e00.env.yaml"), "variables: [1]\n").unwrap();

    let list = f.vars.list_environments(&f.store, &f.id).unwrap();

    assert_eq!(list.environments.len(), 50);
    assert!(list
        .environments
        .iter()
        .any(|e| e.env == "e00" && e.invalid));
    assert!(f
        .vars
        .select_environment(&f.store, &f.id, Some("e00"))
        .is_err());
    assert!(f
        .vars
        .create_environment(&mut f.store, &f.id, "More")
        .is_err());
}

#[test]
fn current_overrides_initial_and_reset_restores_it() {
    let mut f = fixture();
    let dev = f.create("Dev");
    f.vars
        .select_environment(&f.store, &f.id, Some(&dev))
        .unwrap();
    let scope = f.env_scope(&dev);
    let with_current = VariableInput {
        current: Some("http://localhost".into()),
        ..plain("base", "https://dev.example.com")
    };

    let written = f.write(&scope, &[with_current]);
    let yaml = fs::read_to_string(f.root.join("environments/dev.env.yaml")).unwrap();

    assert_eq!(written[0].current.as_deref(), Some("http://localhost"));
    assert!(!yaml.contains("localhost"));
    assert_eq!(f.resolve("{{base}}").0, "http://localhost");

    f.write(&scope, &[plain("base", "https://dev.example.com")]);
    assert_eq!(f.resolve("{{base}}").0, "https://dev.example.com");
}

#[test]
fn precedence_scratch_globals_effective_and_preview() {
    let mut f = fixture();
    let dev = f.create("Dev");
    f.write(
        &VarScope::Global,
        &[plain("host", "g"), plain("only_global", "1")],
    );
    f.write(&f.collection_scope(), &[plain("host", "c")]);
    f.write(
        &f.env_scope(&dev),
        &[plain("host", "e"), plain("url", "https://{{host}}")],
    );

    assert_eq!(f.resolve("{{host}}").0, "c");
    f.vars
        .select_environment(&f.store, &f.id, Some(&dev))
        .unwrap();
    assert_eq!(f.resolve("{{url}}").0, "https://e");

    let scratch = RequestLocation::Scratch { id: "s".into() };
    let scopes = f.vars.scopes(&f.vars.snapshot(&f.store, &scratch));
    let mut report = Report::default();
    assert_eq!(
        resolve_text(
            "{{host}}{{url}}",
            &scopes,
            &FixedDynamics::default(),
            &mut report
        ),
        "g{{url}}"
    );

    let scopes = f.vars.scopes(&f.vars.snapshot(&f.store, &f.location()));
    let effective = effective_variables(&scopes);
    let url = effective.iter().find(|v| v.name == "url").unwrap();
    assert_eq!(
        (url.value.as_str(), url.source),
        ("https://e", VarSource::Environment)
    );
    assert!(effective
        .iter()
        .any(|v| v.name == "$uuid" && v.source == VarSource::Dynamic));
    let shown = preview(&scopes, "{{url}}/{{nope}}").unwrap();
    assert_eq!(shown.text, "https://e/{{nope}}");
    assert_eq!(shown.unresolved, ["nope"]);
    assert!(preview(&scopes, &"x".repeat(64 * 1024 + 1)).is_err());
}

#[test]
fn duplicate_and_invalid_names_are_refused() {
    let mut f = fixture();
    for rows in [
        vec![plain("a", "1"), plain("a", "2")],
        vec![plain("$a", "1")],
        vec![plain("a{", "1")],
    ] {
        let error = f
            .vars
            .write_variables(&mut f.store, &VarScope::Global, &rows)
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput);
    }
}

#[test]
fn selection_persists_and_reopening_keeps_id_and_values() {
    let mut f = fixture();
    let dev = f.create("Dev");
    f.vars
        .select_environment(&f.store, &f.id, Some(&dev))
        .unwrap();
    let with_current = VariableInput {
        current: Some("local".into()),
        ..plain("tenant", "acme")
    };
    f.write(&f.collection_scope(), &[with_current]);

    f.store.close_collection(&f.id).unwrap();
    let mut store = Store::new(&f.temp.path().join("app")).unwrap();
    let reopened = store.open_collection(&f.root).unwrap();
    let vars = VarsService::new(f.vault.clone());

    assert_eq!(reopened.collection.id, f.id);
    assert_eq!(
        vars.list_environments(&store, &f.id).unwrap().selected,
        Some(dev)
    );
    let rows = vars.read_variables(&store, &f.collection_scope()).unwrap();
    assert_eq!(rows[0].current.as_deref(), Some("local"));
    let _ = &mut store;
}

#[test]
fn outside_env_edit_fires_one_change_and_own_writes_none() {
    let mut f = fixture();
    let dev = f.create("Dev");
    let store = Arc::new(Mutex::new(Store::new(&f.temp.path().join("app")).unwrap()));
    store.lock().unwrap().list_collections();
    let (sender, events) = channel();
    let _watch = watcher::watch(&f.root, f.id.clone(), store.clone(), move |id: &str| {
        let _ = sender.send(id.to_string());
    })
    .unwrap();

    let scope = f.env_scope(&dev);
    f.vars
        .write_variables(&mut store.lock().unwrap(), &scope, &[plain("a", "1")])
        .unwrap();
    assert!(
        events.recv_timeout(Duration::from_secs(1)).is_err(),
        "own write fired"
    );

    fs::write(
        f.root.join("environments/dev.env.yaml"),
        "name: Dev\nvariables:\n  a:\n    value: \"2\"\n",
    )
    .unwrap();
    assert_eq!(events.recv_timeout(Duration::from_secs(5)).unwrap(), f.id);
    assert!(
        events.recv_timeout(Duration::from_secs(1)).is_err(),
        "more than one event"
    );
    let _ = &mut f.store;
}
