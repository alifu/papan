//! Plan 13 §6: a journey through the real HTTP pipeline against the local echo server.

mod support;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use papan_lib::engine::http::scripted::SendDeps;
use papan_lib::engine::http::HttpEngine;
use papan_lib::journey::host::{CollectionRef, EngineHost};
use papan_lib::journey::model::{
    Check, CheckOp, CheckSource, Journey, JourneyEdge, JourneyNode, JourneySettings, NodeType,
    Output,
};
use papan_lib::journey::runner::run;
use papan_lib::journey::types::{RunEvent, RunEventKind, RunStatus};
use papan_lib::mock::model::MockEnvironment;
use papan_lib::mock::types::MockLogEntry;
use papan_lib::mock::{MockService, MockSink};
use papan_lib::store::types::{RequestContent, RequestKind, RequestLocation};
use papan_lib::store::{Store, StoreState};
use papan_lib::vars::dynamic::SystemDynamics;
use papan_lib::vars::types::{SecretEdit, VarScope, VariableInput};
use papan_lib::vars::vault::MemoryVault;
use papan_lib::vars::VarsService;

fn node(id: &str, node_type: NodeType) -> JourneyNode {
    JourneyNode::new(id, node_type)
}

fn edge(id: &str, from: &str, to: &str) -> JourneyEdge {
    JourneyEdge {
        id: id.into(),
        from: from.into(),
        to: to.into(),
        when: String::new(),
        map: Vec::new(),
    }
}

fn request_node(id: &str, path: &str) -> JourneyNode {
    JourneyNode {
        request: path.into(),
        ..node(id, NodeType::Request)
    }
}

struct World {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
    engine: HttpEngine,
    store: StoreState,
    vars: VarsService,
    collection_id: String,
}

fn world(base: &str) -> World {
    let dir = tempfile::tempdir().unwrap();
    let engine = HttpEngine::new(&dir.path().join("cache"), dir.path(), "9.9.9").unwrap();
    let mut store = Store::new(&dir.path().join("data")).unwrap();
    let id = store
        .create_collection(dir.path(), "C")
        .unwrap()
        .collection
        .id;
    let vars = VarsService::new(Arc::new(MemoryVault::default()));
    vars.write_variables(
        &mut store,
        &VarScope::Global,
        &[VariableInput {
            name: "base".into(),
            previous_name: None,
            value: base.into(),
            current: None,
            enabled: true,
            secret: false,
            secret_edit: SecretEdit::Keep,
        }],
    )
    .unwrap();
    World {
        root: store.collection_root(&id).unwrap(),
        _dir: dir,
        engine,
        store: StoreState::new(store),
        vars,
        collection_id: id,
    }
}

impl World {
    fn add_request(&self, name: &str, url: &str) -> String {
        let mut store = self.store.lock_store();
        let change = store
            .create_request(&self.collection_id, "", name, RequestKind::Http)
            .unwrap();
        let path = change.path.unwrap();
        let location = RequestLocation::Collection {
            collection_id: self.collection_id.clone(),
            path: path.clone(),
        };
        let content = RequestContent {
            method: "GET".into(),
            url: url.into(),
            ..RequestContent::default()
        };
        store.write_request(&location, &content).unwrap();
        path
    }

    fn host(&self, events: Arc<Mutex<Vec<RunEvent>>>) -> EngineHost<'_> {
        EngineHost::new(
            SendDeps {
                engine: &self.engine,
                store: &self.store,
                vars: &self.vars,
                dynamics: Arc::new(SystemDynamics),
            },
            CollectionRef {
                id: self.collection_id.clone(),
                root: self.root.clone(),
            },
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(None)),
            Vec::new(),
            true,
            Box::new(move |event| events.lock().unwrap().push(event)),
        )
    }
}

#[tokio::test]
async fn a_value_from_one_request_is_used_by_the_next() {
    let server = support::server::spawn().await;
    let world = world(&server.url(""));
    let first = world.add_request("First", "{{base}}/echo?stage=one");
    let second = world.add_request("Second", "{{base}}/echo?got={{method}}");
    let read_method = JourneyNode {
        outputs: vec![Output {
            name: "method".into(),
            source: CheckSource::Body,
            path: "method".into(),
        }],
        ..request_node("a", &first)
    };
    let check = JourneyNode {
        check: Check {
            source: CheckSource::Body,
            name: "query".into(),
            op: CheckOp::Eq,
            value: "got=GET".into(),
        },
        ..node("check", NodeType::Assert)
    };
    let journey = Journey {
        name: "Chain".into(),
        nodes: vec![
            node("s", NodeType::Start),
            read_method,
            request_node("b", &second),
            check,
        ],
        edges: vec![
            edge("e1", "s", "a"),
            edge("e2", "a", "b"),
            edge("e3", "b", "check"),
        ],
        settings: JourneySettings::default(),
    };
    let events = Arc::new(Mutex::new(Vec::new()));
    let host = world.host(Arc::clone(&events));
    let done = run(&host, &journey, "run-1").await;
    assert_eq!(
        (done.status, done.failures),
        (RunStatus::Passed, 0),
        "{}",
        done.message
    );
    assert_eq!(server.hits(), 2);
    let events = events.lock().unwrap();
    let finished = events
        .iter()
        .filter(|e| e.kind == RunEventKind::NodeFinished)
        .count();
    assert_eq!(finished, 4);
    let a = events
        .iter()
        .find(|e| e.node_id == "a" && e.kind == RunEventKind::NodeFinished)
        .unwrap();
    assert_eq!(a.response.as_ref().unwrap().status, 200);
}

#[tokio::test]
async fn a_missing_request_and_other_kinds_fail_the_node() {
    let server = support::server::spawn().await;
    let world = world(&server.url(""));
    let journey = Journey {
        name: "Gone".into(),
        nodes: vec![
            node("s", NodeType::Start),
            request_node("a", "missing.http.yaml"),
        ],
        edges: vec![edge("e1", "s", "a")],
        settings: JourneySettings::default(),
    };
    let host = world.host(Arc::new(Mutex::new(Vec::new())));
    let done = run(&host, &journey, "run-2").await;
    assert_eq!(done.status, RunStatus::Failed);
    assert_eq!(server.hits(), 0);
}

#[tokio::test]
async fn a_script_node_sets_a_variable_for_the_next_request() {
    let server = support::server::spawn().await;
    let world = world(&server.url(""));
    let path = world.add_request("Use", "{{base}}/echo?k={{k}}");
    let script = JourneyNode {
        script: "pm.variables.set('k', 'from-script');".into(),
        ..node("sc", NodeType::Script)
    };
    let check = JourneyNode {
        check: Check {
            source: CheckSource::Body,
            name: "query".into(),
            op: CheckOp::Eq,
            value: "k=from-script".into(),
        },
        ..node("check", NodeType::Assert)
    };
    let journey = Journey {
        name: "Script".into(),
        nodes: vec![
            node("s", NodeType::Start),
            script,
            request_node("r", &path),
            check,
        ],
        edges: vec![
            edge("e1", "s", "sc"),
            edge("e2", "sc", "r"),
            edge("e3", "r", "check"),
        ],
        settings: JourneySettings::default(),
    };
    let host = world.host(Arc::new(Mutex::new(Vec::new())));
    let done = run(&host, &journey, "run-3").await;
    assert_eq!(done.status, RunStatus::Passed, "{}", done.message);
}

#[tokio::test]
async fn a_secret_the_host_was_not_told_about_is_masked_in_events() {
    let server = support::server::spawn().await;
    let world = world(&server.url(""));
    world
        .vars
        .write_variables(
            &mut world.store.lock_store(),
            &VarScope::Global,
            &[
                VariableInput {
                    name: "base".into(),
                    previous_name: None,
                    value: server.url(""),
                    current: None,
                    enabled: true,
                    secret: false,
                    secret_edit: SecretEdit::Keep,
                },
                VariableInput {
                    name: "tok".into(),
                    previous_name: None,
                    value: "s3cretvalue".into(),
                    current: None,
                    enabled: true,
                    secret: true,
                    secret_edit: SecretEdit::Set {
                        value: "s3cretvalue".into(),
                    },
                },
            ],
        )
        .unwrap();
    let path = world.add_request("Echo", "{{base}}/echo?k={{tok}}");
    let journey = Journey {
        name: "Secret".into(),
        nodes: vec![node("s", NodeType::Start), request_node("a", &path)],
        edges: vec![edge("e1", "s", "a")],
        settings: JourneySettings::default(),
    };
    let events = Arc::new(Mutex::new(Vec::new()));
    // The host starts with no secrets, as if they lived in a folder it did not read.
    let host = world.host(Arc::clone(&events));
    let done = run(&host, &journey, "run-s").await;
    assert_eq!(done.status, RunStatus::Passed, "{}", done.message);
    let events = events.lock().unwrap();
    let text = serde_json::to_string(&*events).unwrap();
    assert!(!text.contains("s3cretvalue"), "{text}");
    assert!(text.contains("k="), "the echo reached the event: {text}");
}

struct Quiet;
impl MockSink for Quiet {
    fn logged(&self, _collection_id: &str, _file: &str, _entry: &MockLogEntry) {}
}

fn mock_with_routes() -> MockEnvironment {
    let route = |id: &str, endpoint: &str, body: &str| {
        serde_json::json!({
            "id": id, "method": "get", "endpoint": endpoint,
            "responses": [{ "id": "r", "status": 200, "body": body, "default": true }],
        })
    };
    serde_json::from_value(serde_json::json!({
        "name": "Api",
        "routes": [
            route("login", "login", r#"{"data":{"token":"abc123"}}"#),
            route("whoami", "whoami", r#"{"seen":"{{queryParam 'token'}}"}"#),
            route("user", "users/:id", r#"{"id":"{{urlParam 'id'}}"}"#),
        ],
    }))
    .unwrap()
}

#[tokio::test]
async fn a_journey_runs_against_a_mock_with_a_mapped_token_and_a_data_file_loop() {
    let mocks = MockService::new(Arc::new(Quiet));
    let dir = tempfile::tempdir().unwrap();
    mocks
        .start("m", "api.mock.yaml", dir.path().into(), mock_with_routes())
        .await
        .unwrap();
    let base = mocks.url_of("m", "api.mock.yaml").await.unwrap();
    let world = world(&base);
    std::fs::create_dir_all(world.root.join("data")).unwrap();
    std::fs::write(world.root.join("data/ids.csv"), "id\n1\n2\n").unwrap();
    let login = world.add_request("Login", "{{base}}/login");
    let whoami = world.add_request("Whoami", "{{base}}/whoami?token={{auth}}");
    let user = world.add_request("User", "{{base}}/users/{{id}}");
    let check = JourneyNode {
        check: Check {
            source: CheckSource::Body,
            name: "seen".into(),
            op: CheckOp::Eq,
            value: "abc123".into(),
        },
        ..node("check", NodeType::Assert)
    };
    let each_id = JourneyNode {
        data_file: "data/ids.csv".into(),
        ..node("each", NodeType::Loop)
    };
    let mut to_whoami = edge("e2", "login", "whoami");
    to_whoami.map = vec![Output {
        name: "auth".into(),
        source: CheckSource::Body,
        path: "data.token".into(),
    }];
    let journey = Journey {
        name: "Mocked".into(),
        nodes: vec![
            node("s", NodeType::Start),
            request_node("login", &login),
            request_node("whoami", &whoami),
            check,
            each_id,
            request_node("user", &user),
        ],
        edges: vec![
            edge("e1", "s", "login"),
            to_whoami,
            edge("e3", "whoami", "check"),
            edge("e4", "check", "each"),
            JourneyEdge {
                when: "each".into(),
                ..edge("e5", "each", "user")
            },
            edge("e6", "user", "each"),
        ],
        settings: JourneySettings::default(),
    };
    papan_lib::journey::model::validate("t", &journey).unwrap();
    let host = world.host(Arc::new(Mutex::new(Vec::new())));
    let done = run(&host, &journey, "run-3").await;
    assert_eq!(done.status, RunStatus::Passed, "{}", done.message);
    let paths: Vec<String> = mocks
        .log("m", "api.mock.yaml")
        .await
        .iter()
        .map(|entry| entry.path.clone())
        .collect();
    assert_eq!(paths, ["/login", "/whoami", "/users/1", "/users/2"]);
    mocks.stop_all().await;
}
