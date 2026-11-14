//! Sending with variables against the local echo server (plan 04 §6, §9).

mod support;

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use papan_lib::engine::http::scripted::{run_send, SendDeps};
use papan_lib::engine::http::types::ResponseMeta;
use papan_lib::engine::http::HttpEngine;
use papan_lib::error::{AppError, ErrorCode};
use papan_lib::store::types::{
    ApiKeyPlacement, Auth, Body, KeyValue, Part, RequestContent, RequestLocation,
};
use papan_lib::store::{Store, StoreState};
use papan_lib::vars::dynamic::SystemDynamics;
use papan_lib::vars::types::{SecretEdit, VarScope, VariableInput};
use papan_lib::vars::vault::MemoryVault;
use papan_lib::vars::VarsService;
use serde_json::Value;
use support::server::{self, TestServer};

const SECRET: &str = "SeCrEt-4410";

struct Fixture {
    dir: tempfile::TempDir,
    engine: HttpEngine,
    store: StoreState,
    vars: VarsService,
    collection: RequestLocation,
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
    let vars = VarsService::new(Arc::new(MemoryVault::default()));
    vars.write_variables(
        &mut store,
        &VarScope::Global,
        &[
            row("base", &server.url(""), false),
            row("who", "global", false),
        ],
    )
    .unwrap();
    vars.write_variables(
        &mut store,
        &VarScope::Collection {
            collection_id: id.clone(),
        },
        &[
            row("who", "collection", false),
            row("token", SECRET, true),
            row("crlf", "a\r\nInjected: 1", false),
        ],
    )
    .unwrap();
    Fixture {
        dir,
        engine,
        store: StoreState::new(store),
        vars,
        collection: RequestLocation::Collection {
            collection_id: id,
            path: "r.http.yaml".into(),
        },
    }
}

impl Fixture {
    async fn send(
        &self,
        location: &RequestLocation,
        content: &RequestContent,
    ) -> (String, Result<ResponseMeta, AppError>) {
        let id = uuid::Uuid::new_v4().to_string();
        let deps = SendDeps {
            engine: &self.engine,
            store: &self.store,
            vars: &self.vars,
            dynamics: Arc::new(SystemDynamics),
        };
        let result = run_send(&deps, &id, location, content)
            .await
            .map(|outcome| outcome.meta.expect("no script stops these sends"));
        (id, result)
    }

    async fn echo(&self, location: &RequestLocation, content: &RequestContent) -> Value {
        let (id, result) = self.send(location, content).await;
        result.unwrap();
        serde_json::from_str(&self.engine.read_body(&id).unwrap().data).unwrap()
    }

    fn history_text(&self) -> String {
        serde_json::to_string(&self.store.lock_store().list_history()).unwrap()
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

fn body(echo: &Value) -> String {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(echo["body"].as_str().unwrap())
        .unwrap();
    String::from_utf8(bytes).unwrap()
}

fn kv(name: &str, value: &str) -> KeyValue {
    KeyValue {
        name: name.into(),
        value: value.into(),
        enabled: true,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn variables_arrive_resolved_everywhere_they_are_sent() {
    let server = server::spawn().await;
    let f = fixture(&server);
    let json = RequestContent {
        headers: vec![kv("X-Who", "{{who}}")],
        body: Body::Json {
            text: r#"{"who":"{{who}}","id":"{{$uuid}}","n":"{{nope}}"}"#.into(),
        },
        auth: Auth::Bearer {
            token: "{{token}}".into(),
        },
        ..RequestContent::new("POST", "{{base}}/echo?q={{who}}")
    };

    let echoed = f.echo(&f.collection, &json).await;
    let (_, meta) = f.send(&f.collection, &json).await;

    assert_eq!(echoed["query"], "q=collection");
    assert_eq!(header(&echoed, "x-who").as_deref(), Some("collection"));
    assert_eq!(
        header(&echoed, "authorization"),
        Some(format!("Bearer {SECRET}"))
    );
    let sent: Value = serde_json::from_str(&body(&echoed)).unwrap();
    assert_eq!(sent["who"], "collection");
    assert_eq!(sent["n"], "{{nope}}");
    assert!(uuid::Uuid::parse_str(sent["id"].as_str().unwrap()).is_ok());
    assert_eq!(meta.unwrap().unresolved, ["nope"]);

    let form = RequestContent {
        body: Body::Form {
            fields: vec![kv("{{who}}", "{{who}}")],
        },
        auth: Auth::Basic {
            username: "{{who}}".into(),
            password: "{{token}}".into(),
        },
        ..RequestContent::new("POST", "{{base}}/echo")
    };
    let echoed = f.echo(&f.collection, &form).await;
    assert_eq!(body(&echoed), "collection=collection");
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("collection:{SECRET}"));
    assert_eq!(
        header(&echoed, "authorization"),
        Some(format!("Basic {basic}"))
    );

    let multipart = RequestContent {
        body: Body::Multipart {
            parts: vec![Part {
                name: "p".into(),
                value: "{{who}}".into(),
                file: None,
                enabled: true,
            }],
        },
        auth: Auth::ApiKey {
            name: "X-{{who}}".into(),
            value: "{{token}}".into(),
            placement: ApiKeyPlacement::Header,
        },
        ..RequestContent::new("POST", "{{base}}/echo")
    };
    let echoed = f.echo(&f.collection, &multipart).await;
    let encoded = base64::engine::general_purpose::STANDARD.encode("collection");
    assert_eq!(echoed["parts"][0]["body"], encoded);
    assert_eq!(header(&echoed, "x-collection").as_deref(), Some(SECRET));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_resolved_crlf_header_is_refused_before_sending() {
    let server = server::spawn().await;
    let f = fixture(&server);
    let content = RequestContent {
        headers: vec![kv("X-Bad", "{{crlf}}")],
        ..RequestContent::new("GET", "{{base}}/echo")
    };

    let (_, result) = f.send(&f.collection, &content).await;

    assert_eq!(result.unwrap_err().code, ErrorCode::InvalidInput);
    assert_eq!(server.hits(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_header_built_from_a_secret_is_not_forwarded_cross_origin() {
    let first = server::spawn().await;
    let second = server::spawn().await;
    let f = fixture(&first);
    let content = RequestContent {
        headers: vec![kv("X-Plain-Name", "t={{token}}"), kv("X-Keep", "{{who}}")],
        ..RequestContent::new(
            "GET",
            first.url(&format!("/redirect-to?url={}", second.url("/echo"))),
        )
    };

    let echoed = f.echo(&f.collection, &content).await;

    assert_eq!(header(&echoed, "x-plain-name"), None);
    assert_eq!(header(&echoed, "x-keep").as_deref(), Some("collection"));
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn secrets_stay_out_of_history_logs_errors_and_the_shown_url() {
    let server = server::spawn().await;
    let f = fixture(&server);
    let capture = Capture::default();
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .finish();
    // Global, so every test in this file logs into the capture.
    tracing::subscriber::set_global_default(subscriber).unwrap();
    let in_query = RequestContent::new("GET", "{{base}}/echo?k={{token}}&w={{who}}");
    let refused = RequestContent::new("GET", "http://127.0.0.1:1/{{token}}");

    let (_, ok) = f.send(&f.collection, &in_query).await;
    let (_, failed) = f.send(&f.collection, &refused).await;

    let meta = ok.unwrap();
    assert!(!meta.url.contains(SECRET), "{}", meta.url);
    assert!(meta.url.contains("w=collection"));
    let error = failed.unwrap_err();
    let error_text = format!("{} {:?}", error.message, error.detail);
    assert!(!error_text.contains(SECRET), "{error_text}");
    let history = f.history_text();
    assert!(history.contains("{{token}}") && !history.contains(SECRET));
    assert!(
        !history.contains("collection"),
        "history holds a resolved value"
    );
    let logs = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("sending request"), "nothing captured");
    assert!(
        logs.contains("{{base}}/echo"),
        "the log holds a resolved URL: {logs}"
    );
    assert!(
        !logs.contains("w=") && !logs.contains(&server.url("")),
        "{logs}"
    );
    assert!(!logs.contains(SECRET), "{logs}");
    let on_disk = files_containing(f.dir.path(), SECRET);
    assert!(on_disk.is_empty(), "{on_disk:?}");
}

fn files_containing(dir: &std::path::Path, needle: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.ends_with("cache") {
                continue;
            }
            found.extend(files_containing(&path, needle));
        } else if String::from_utf8_lossy(&std::fs::read(&path).unwrap()).contains(needle) {
            found.push(path);
        }
    }
    found
}

#[tokio::test(flavor = "multi_thread")]
async fn scratch_sees_globals_only_and_a_broken_url_names_the_variable() {
    let server = server::spawn().await;
    let f = fixture(&server);
    let scratch = RequestLocation::Scratch { id: "s".into() };
    let content = RequestContent::new("GET", "{{base}}/echo?w={{who}}&t={{token}}");

    let echoed = f.echo(&scratch, &content).await;
    let (_, broken) = f
        .send(&scratch, &RequestContent::new("GET", "{{missing}}/x"))
        .await;

    assert_eq!(echoed["query"], "w=global&t={{token}}");
    let error = broken.unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(error.message.contains("`missing`"), "{}", error.message);
}

#[tokio::test(flavor = "multi_thread")]
async fn percent_encoded_secrets_are_masked_in_the_url_and_errors() {
    let server = server::spawn().await;
    let f = fixture(&server);
    let encoded_secret = "a\"b/c é!x";
    f.vars
        .write_variables(
            &mut f.store.lock_store(),
            &VarScope::Global,
            &[
                row("base", &server.url(""), false),
                row("odd", encoded_secret, true),
            ],
        )
        .unwrap();
    let scratch = RequestLocation::Scratch { id: "s".into() };

    let (_, ok) = f
        .send(
            &scratch,
            &RequestContent::new("GET", "{{base}}/echo?k={{odd}}"),
        )
        .await;
    let (_, failed) = f
        .send(
            &scratch,
            &RequestContent::new("GET", "http://127.0.0.1:1/p/{{odd}}"),
        )
        .await;

    let url = ok.unwrap().url;
    assert!(
        url.contains("[REDACTED]") && !url.contains("a%22b"),
        "{url}"
    );
    let error = failed.unwrap_err();
    let text = format!("{} {:?}", error.message, error.detail);
    assert!(!text.contains("%22b") && !text.contains("C3%A9"), "{text}");
}
