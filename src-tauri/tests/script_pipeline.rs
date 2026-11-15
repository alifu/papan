//! Plan 05 §4.2, §8: scripts around a send, against the local echo server.

mod support;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use papan_lib::engine::http::scripted::{run_send, SendDeps};
use papan_lib::engine::http::types::SendOutcome;
use papan_lib::engine::http::HttpEngine;
use papan_lib::error::{AppError, ErrorCode};
use papan_lib::script::{ConsoleLevel, ScriptPhase};
use papan_lib::store::types::{
    Body, KeyValue, RequestContent, RequestLocation, ScriptTarget, Scripts,
};
use papan_lib::store::{Store, StoreState};
use papan_lib::vars::dynamic::SystemDynamics;
use papan_lib::vars::types::{SecretEdit, VarScope, Variable, VariableInput};
use papan_lib::vars::vault::{MemoryVault, SecretStore};
use papan_lib::vars::VarsService;
use serde_json::Value;
use support::server::{self, TestServer};

const VAULT_SECRET: &str = "Vault-Only-5521";

struct Fixture {
    dir: tempfile::TempDir,
    engine: HttpEngine,
    store: StoreState,
    vars: VarsService,
    vault: Arc<MemoryVault>,
    collection_id: String,
    root: std::path::PathBuf,
    env: VarScope,
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
    let tree = store.create_collection(dir.path(), "C").unwrap();
    let id = tree.collection.id;
    let root = std::path::PathBuf::from(tree.collection.path);
    store.create_folder(&id, "", "Auth").unwrap();
    let vault = Arc::new(MemoryVault::default());
    let vars = VarsService::new(vault.clone());
    vars.write_variables(
        &mut store,
        &VarScope::Global,
        &[
            row("base", &server.url(""), false),
            row("who", "global", false),
        ],
    )
    .unwrap();
    let list = vars.create_environment(&mut store, &id, "Dev").unwrap();
    let env_name = list.environments[0].env.clone();
    vars.select_environment(&store, &id, Some(&env_name))
        .unwrap();
    let env = VarScope::Environment {
        collection_id: id.clone(),
        env: env_name,
    };
    vars.write_variables(&mut store, &env, &[row("key", VAULT_SECRET, true)])
        .unwrap();
    Fixture {
        dir,
        engine,
        store: StoreState::new(store),
        vars,
        vault,
        collection_id: id,
        root,
        env,
    }
}

impl Fixture {
    fn location(&self) -> RequestLocation {
        RequestLocation::Collection {
            collection_id: self.collection_id.clone(),
            path: "auth/r.http.yaml".into(),
        }
    }

    async fn send_with_id(
        &self,
        id: &str,
        location: &RequestLocation,
        content: &RequestContent,
    ) -> Result<SendOutcome, AppError> {
        let deps = SendDeps {
            engine: &self.engine,
            store: &self.store,
            vars: &self.vars,
            dynamics: Arc::new(SystemDynamics),
        };
        run_send(&deps, id, location, content).await
    }

    async fn send(&self, content: &RequestContent) -> (String, Result<SendOutcome, AppError>) {
        let id = uuid::Uuid::new_v4().to_string();
        let result = self.send_with_id(&id, &self.location(), content).await;
        (id, result)
    }

    fn echo(&self, id: &str) -> Value {
        serde_json::from_str(&self.engine.read_body(id).unwrap().data).unwrap()
    }

    fn variable(&self, scope: &VarScope, name: &str) -> Option<Variable> {
        let guard = self.store.lock_store();
        let all = self.vars.read_variables(&guard, scope).unwrap();
        all.into_iter().find(|v| v.name == name)
    }

    fn history_text(&self) -> String {
        serde_json::to_string(&self.store.lock_store().list_history()).unwrap()
    }
}

fn kv(name: &str, value: &str) -> KeyValue {
    KeyValue {
        name: name.into(),
        value: value.into(),
        enabled: true,
    }
}

fn scripts(pre: &str, post: &str) -> Scripts {
    Scripts {
        enabled: true,
        pre_request: pre.into(),
        post_response: post.into(),
    }
}

fn header(echo: &Value, name: &str) -> Option<String> {
    echo["headers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str().unwrap().to_string())
}

fn files_containing(dir: &Path, needle: &str) -> Vec<String> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(files_containing(&path, needle));
        } else if let Ok(bytes) = std::fs::read(&path) {
            if String::from_utf8_lossy(&bytes).contains(needle) {
                found.push(path.display().to_string());
            }
        }
    }
    found
}

#[tokio::test(flavor = "multi_thread")]
async fn a_token_from_the_login_response_is_used_by_the_next_send() {
    let server = server::spawn().await;
    let f = fixture(&server);
    let login = RequestContent {
        body: Body::Json {
            text: r#"{"token":"tok-123"}"#.into(),
        },
        scripts: scripts(
            "",
            r#"const sent = JSON.parse(atob(pm.response.json().body));
pm.environment.set("token", sent.token);
pm.test("logged in", () => pm.response.to.have.status(200));"#,
        ),
        ..RequestContent::new("POST", "{{base}}/echo")
    };
    let me = RequestContent {
        headers: vec![kv("Authorization", "Bearer {{token}}")],
        ..RequestContent::new("GET", "{{base}}/echo")
    };

    let (_, first) = f.send(&login).await;
    let (id, second) = f.send(&me).await;

    let first = first.unwrap();
    assert_eq!(first.scripts.len(), 1);
    let report = &first.scripts[0];
    assert_eq!(report.phase, ScriptPhase::PostResponse);
    assert!(report.tests[0].passed);
    assert_eq!(report.changes[0].name, "token");
    assert!(second.unwrap().scripts.is_empty());
    assert_eq!(
        header(&f.echo(&id), "authorization").as_deref(),
        Some("Bearer tok-123")
    );
    let token = f.variable(&f.env, "token").unwrap();
    assert_eq!(
        (token.value.as_str(), token.current.as_deref()),
        ("", Some("tok-123"))
    );
    let in_git: Vec<String> = files_containing(&f.root, "tok-123");
    assert!(
        in_git.is_empty(),
        "token reached the collection: {in_git:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn chains_run_outer_to_inner_and_a_pre_request_script_changes_only_the_sent_copy() {
    let server = server::spawn().await;
    let f = fixture(&server);
    {
        let mut guard = f.store.lock_store();
        let collection = ScriptTarget::Collection {
            collection_id: f.collection_id.clone(),
        };
        let folder = ScriptTarget::Folder {
            collection_id: f.collection_id.clone(),
            path: "auth".into(),
        };
        guard
            .write_scripts(
                &collection,
                &scripts("pm.variables.set(\"order\", \"c\");", ""),
            )
            .unwrap();
        guard
            .write_scripts(
                &folder,
                &scripts(
                    "pm.variables.set(\"order\", pm.variables.get(\"order\") + \"f\");",
                    "",
                ),
            )
            .unwrap();
    }
    let draft = RequestContent {
        headers: vec![kv("X-Order", "{{order}}")],
        scripts: scripts(
            r#"pm.request.headers.add("X-Sig", "sig-" + pm.variables.get("who"));
pm.variables.set("order", pm.variables.get("order") + "r");"#,
            "",
        ),
        ..RequestContent::new("GET", "{{base}}/echo")
    };
    let before = draft.clone();

    let (id, outcome) = f.send(&draft).await;

    let outcome = outcome.unwrap();
    assert_eq!(outcome.scripts[0].phase, ScriptPhase::PreRequest);
    let echo = f.echo(&id);
    assert_eq!(header(&echo, "x-sig").as_deref(), Some("sig-global"));
    assert_eq!(header(&echo, "x-order").as_deref(), Some("cfr"));
    assert_eq!(draft, before);
    let history = f.history_text();
    assert!(!history.contains("sig-global"), "{history}");
    assert_eq!(f.variable(&f.env, "order"), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_disabled_level_is_skipped_without_an_error_and_the_others_still_run() {
    let server = server::spawn().await;
    let f = fixture(&server);
    {
        let mut guard = f.store.lock_store();
        let collection = ScriptTarget::Collection {
            collection_id: f.collection_id.clone(),
        };
        let folder = ScriptTarget::Folder {
            collection_id: f.collection_id.clone(),
            path: "auth".into(),
        };
        let off = Scripts {
            enabled: false,
            ..scripts(
                "pm.variables.set(\"order\", \"BAD\"); throw new Error(\"no\");",
                "",
            )
        };
        guard.write_scripts(&collection, &off).unwrap();
        guard
            .write_scripts(&folder, &scripts("pm.variables.set(\"order\", \"f\");", ""))
            .unwrap();
    }
    let draft = RequestContent {
        headers: vec![kv("X-Order", "{{order}}")],
        scripts: Scripts {
            enabled: false,
            ..scripts("pm.variables.set(\"order\", \"BAD2\");", "")
        },
        ..RequestContent::new("GET", "{{base}}/echo")
    };

    let (id, outcome) = f.send(&draft).await;

    let outcome = outcome.unwrap();
    assert!(
        outcome.scripts.iter().all(|r| r.error.is_none()),
        "{:?}",
        outcome.scripts
    );
    assert_eq!(header(&f.echo(&id), "x-order").as_deref(), Some("f"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_post_response_error_keeps_the_response_and_earlier_changes() {
    let server = server::spawn().await;
    let f = fixture(&server);
    let draft = RequestContent {
        scripts: scripts(
            "",
            "pm.environment.set(\"before\", \"1\");\nthrow new Error(\"boom\");",
        ),
        ..RequestContent::new("GET", "{{base}}/status/404")
    };

    let (_, outcome) = f.send(&draft).await;

    let outcome = outcome.unwrap();
    assert_eq!(outcome.meta.unwrap().status, 404);
    let error = outcome.scripts[0].error.as_ref().unwrap();
    assert!(error.message.contains("boom"));
    let before = f.variable(&f.env, "before").unwrap();
    assert_eq!(before.current.as_deref(), Some("1"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pre_request_error_sends_nothing_but_returns_the_report() {
    let server = server::spawn().await;
    let f = fixture(&server);
    let draft = RequestContent {
        scripts: scripts(
            "pm.globals.set(\"tried\", \"yes\");\nthrow new Error(\"nope\");",
            "pm.globals.set(\"post\", \"ran\");",
        ),
        ..RequestContent::new("GET", "{{base}}/echo")
    };

    let (_, outcome) = f.send(&draft).await;

    let outcome = outcome.unwrap();
    assert!(outcome.meta.is_none());
    assert_eq!(outcome.scripts.len(), 1);
    assert!(outcome.scripts[0]
        .error
        .as_ref()
        .unwrap()
        .message
        .contains("nope"));
    assert_eq!(server.hits(), 0);
    assert_eq!(f.history_text(), "[]");
    assert_eq!(f.variable(&VarScope::Global, "tried").unwrap().value, "yes");
    assert_eq!(f.variable(&VarScope::Global, "post"), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_secret_set_by_a_script_goes_to_the_vault_and_never_to_disk_or_the_report() {
    let server = server::spawn().await;
    let f = fixture(&server);
    let new_secret = "Rotated-Secret-9931";
    let encoded = base64::engine::general_purpose::STANDARD.encode(VAULT_SECRET);
    let draft = RequestContent {
        scripts: scripts(
            "",
            "console.log(pm.environment.get(\"key\"), btoa(pm.environment.get(\"key\")));\n\
                 pm.environment.set(\"key\", \"Rotated-\" + \"Secret-9931\");\n\
                 console.log(\"now\", pm.environment.get(\"key\"));",
        ),
        ..RequestContent::new("GET", "{{base}}/echo")
    };

    let (_, outcome) = f.send(&draft).await;

    let report = &outcome.unwrap().scripts[0];
    let text = serde_json::to_string(report).unwrap();
    for leaked in [VAULT_SECRET, new_secret, encoded.as_str()] {
        assert!(!text.contains(leaked), "{leaked} in {text}");
    }
    assert!(report.changes[0].secret && report.changes[0].value.is_none());
    let VarScope::Environment { collection_id, env } = &f.env else {
        unreachable!()
    };
    let vault_id = format!("e/{collection_id}/{env}/key");
    assert_eq!(f.vault.get(&vault_id).unwrap().as_deref(), Some(new_secret));
    assert!(files_containing(f.dir.path(), new_secret).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn pm_send_request_goes_through_the_engine() {
    let server = server::spawn().await;
    let f = fixture(&server);
    let draft = RequestContent {
        headers: vec![kv("X-From", "{{echoed}}")],
        scripts: scripts(
            r#"pm.sendRequest({ url: pm.variables.get("base") + "/echo?from=script", method: "POST",
  body: { mode: "raw", raw: "hi" } }, (err, res) => {
  if (err) throw err;
  const echo = res.json();
  pm.variables.set("echoed", echo.query + ":" + atob(echo.body) + ":" + res.code);
});"#,
            "",
        ),
        ..RequestContent::new("GET", "{{base}}/echo")
    };

    let (id, outcome) = f.send(&draft).await;

    assert!(outcome.unwrap().scripts[0].error.is_none());
    assert_eq!(server.hits(), 2);
    assert_eq!(
        header(&f.echo(&id), "x-from").as_deref(),
        Some("from=script:hi:200")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_stops_a_running_script_and_sends_nothing() {
    let server = server::spawn().await;
    let f = Arc::new(fixture(&server));
    let draft = RequestContent {
        scripts: scripts("while (true) {}", ""),
        ..RequestContent::new("GET", "{{base}}/echo")
    };
    let id = uuid::Uuid::new_v4().to_string();
    let started = Instant::now();

    let running = {
        let f = Arc::clone(&f);
        let id = id.clone();
        tokio::spawn(async move { f.send_with_id(&id, &f.location(), &draft).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    f.engine.cancel(&id).unwrap();
    let result = running.await.unwrap();

    assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(server.hits(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_scratch_request_runs_its_own_scripts_only() {
    let server = server::spawn().await;
    let f = fixture(&server);
    {
        let mut guard = f.store.lock_store();
        let collection = ScriptTarget::Collection {
            collection_id: f.collection_id.clone(),
        };
        guard
            .write_scripts(
                &collection,
                &scripts("", "pm.globals.set(\"collection\", \"ran\");"),
            )
            .unwrap();
    }
    let draft = RequestContent {
        scripts: scripts(
            "",
            "pm.globals.set(\"seen\", pm.response.code);\npm.collectionVariables.set(\"c\", \"1\");",
        ),
        ..RequestContent::new("GET", "{{base}}/echo")
    };
    let scratch = RequestLocation::Scratch {
        id: uuid::Uuid::new_v4().to_string(),
    };
    let id = uuid::Uuid::new_v4().to_string();

    let outcome = f.send_with_id(&id, &scratch, &draft).await.unwrap();

    assert_eq!(f.variable(&VarScope::Global, "seen").unwrap().value, "200");
    assert_eq!(f.variable(&VarScope::Global, "collection"), None);
    let console = &outcome.scripts[0].console;
    assert!(console
        .iter()
        .any(|line| line.level == ConsoleLevel::Warn && line.text.contains("`c`")));
}
