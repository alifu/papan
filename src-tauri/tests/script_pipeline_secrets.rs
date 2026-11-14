//! Secrets and the response cache around scripted sends, against the local echo server.

mod support;

use std::path::Path;
use std::sync::Arc;

use papan_lib::engine::http::scripted::{run_send, SendDeps};
use papan_lib::engine::http::types::SendOutcome;
use papan_lib::engine::http::HttpEngine;
use papan_lib::script::ConsoleLevel;
use papan_lib::store::types::{RequestContent, RequestLocation, Scripts};
use papan_lib::store::{Store, StoreState};
use papan_lib::vars::dynamic::SystemDynamics;
use papan_lib::vars::types::{SecretEdit, VarScope, VariableInput};
use papan_lib::vars::vault::MemoryVault;
use papan_lib::vars::VarsService;
use support::server::{self, TestServer};

const VAULT_SECRET: &str = "Vault-Only-5521";

struct Fixture {
    dir: tempfile::TempDir,
    engine: HttpEngine,
    store: StoreState,
    vars: VarsService,
    location: RequestLocation,
}

fn row(name: &str, value: &str, secret: bool) -> VariableInput {
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

fn fixture(server: &TestServer) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let engine = HttpEngine::new(&dir.path().join("cache"), dir.path(), "9.9.9").unwrap();
    let mut store = Store::new(&dir.path().join("data")).unwrap();
    let id = store
        .create_collection(dir.path(), "C")
        .unwrap()
        .collection
        .id;
    let vars = VarsService::new(Arc::new(MemoryVault::default()));
    let globals = [row("base", &server.url(""), false)];
    vars.write_variables(&mut store, &VarScope::Global, &globals)
        .unwrap();
    let list = vars.create_environment(&mut store, &id, "Dev").unwrap();
    let env = list.environments[0].env.clone();
    vars.select_environment(&store, &id, Some(&env)).unwrap();
    let scope = VarScope::Environment {
        collection_id: id.clone(),
        env,
    };
    vars.write_variables(&mut store, &scope, &[row("key", VAULT_SECRET, true)])
        .unwrap();
    Fixture {
        dir,
        engine,
        store: StoreState::new(store),
        vars,
        location: RequestLocation::Collection {
            collection_id: id,
            path: "r.http.yaml".into(),
        },
    }
}

impl Fixture {
    async fn send(&self, pre: &str, post: &str) -> SendOutcome {
        let content = RequestContent {
            scripts: Scripts {
                pre_request: pre.into(),
                post_response: post.into(),
            },
            ..RequestContent::new("GET", "{{base}}/echo")
        };
        let deps = SendDeps {
            engine: &self.engine,
            store: &self.store,
            vars: &self.vars,
            dynamics: Arc::new(SystemDynamics),
        };
        let id = uuid::Uuid::new_v4().to_string();
        run_send(&deps, &id, &self.location, &content)
            .await
            .unwrap()
    }
}

fn files_in(dir: &Path) -> Vec<std::path::PathBuf> {
    let entries = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path());
    entries
        .flat_map(|path| {
            if path.is_dir() {
                files_in(&path)
            } else {
                vec![path]
            }
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_secret_a_script_puts_in_the_url_is_masked_in_the_shown_url() {
    let server = server::spawn().await;
    let f = fixture(&server);

    let outcome = f
        .send(
            r#"pm.variables.set("copy", pm.environment.get("key"));
pm.request.url = pm.variables.get("base") + "/echo?a=" + pm.environment.get("key") + "&b={{copy}}";"#,
            r#"pm.test("sent", () => pm.expect(pm.response.json().query).to.include("a=Vault-Only-5521&b=Vault-Only-5521"));"#,
        )
        .await;

    let json = serde_json::to_string(&outcome).unwrap();
    assert!(!json.contains(VAULT_SECRET), "{json}");
    assert!(outcome.scripts[1].tests[0].passed, "the secret was sent");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_secret_copied_to_a_plain_variable_is_not_saved() {
    let server = server::spawn().await;
    let f = fixture(&server);

    let outcome = f
        .send(
            "",
            r#"const key = pm.environment.get("key");
pm.environment.set("copy", key);
pm.globals.set("leak", key);
pm.environment.set("fine", "plain");"#,
        )
        .await;

    let leaked: Vec<_> = files_in(f.dir.path())
        .into_iter()
        .filter(|path| {
            String::from_utf8_lossy(&std::fs::read(path).unwrap()).contains(VAULT_SECRET)
        })
        .collect();
    assert!(leaked.is_empty(), "{leaked:?}");
    let warnings: Vec<&str> = outcome.scripts[0]
        .console
        .iter()
        .filter(|line| line.level == ConsoleLevel::Warn)
        .map(|line| line.text.as_str())
        .collect();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(warnings[0].contains("`copy`") || warnings[1].contains("`copy`"));
}

#[tokio::test(flavor = "multi_thread")]
async fn pm_send_request_takes_no_slot_of_the_response_cache() {
    let server = server::spawn().await;
    let f = fixture(&server);

    let outcome = f
        .send(
            r#"pm.sendRequest(pm.variables.get("base") + "/echo", (err, res) => {
  pm.test("answered", () => pm.expect(res.code).to.equal(200));
});"#,
            "",
        )
        .await;

    assert!(outcome.scripts[0].tests[0].passed);
    assert_eq!(server.hits(), 2);
    let kept = files_in(&f.dir.path().join("cache").join("responses"));
    assert_eq!(kept.len(), 1, "{kept:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_transport_failure_keeps_the_pre_request_report() {
    let server = server::spawn().await;
    let f = fixture(&server);

    let outcome = f
        .send(
            r#"console.log("before the failure"); pm.request.url = "http://127.0.0.1:1/unreachable";"#,
            "",
        )
        .await;

    assert!(outcome.meta.is_none());
    assert!(outcome.error.is_some(), "the failure is reported");
    let lines: Vec<&str> = outcome.scripts[0]
        .console
        .iter()
        .map(|line| line.text.as_str())
        .collect();
    assert!(lines.contains(&"before the failure"), "{lines:?}");
}
