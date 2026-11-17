//! Engine, store and variables for the gRPC integration tests (plan 08 §9): a collection
//! whose `protos/` holds the test service and the greeter fixtures, a global secret
//! `token` held in the vault only, and a recorder of every `GrpcEvent`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use papan_lib::engine::grpc::service::{GrpcDeps, GrpcState};
use papan_lib::engine::grpc::sink::{GrpcBatchingSink, GrpcInbound};
use papan_lib::engine::grpc::types::{
    GrpcEvent, GrpcEventKind, GrpcSchema, GrpcStarted, GrpcStatus,
};
use papan_lib::engine::http::HttpEngine;
use papan_lib::engine::ws::types::{WsDirection, WsMessage};
use papan_lib::error::AppError;
use papan_lib::store::types::{
    GrpcSettings, GrpcSource, KeyValue, RequestContent, RequestKind, RequestLocation,
};
use papan_lib::store::{Store, StoreState};
use papan_lib::vars::dynamic::SystemDynamics;
use papan_lib::vars::types::{SecretEdit, VarScope, VariableInput};
use papan_lib::vars::vault::MemoryVault;
use papan_lib::vars::VarsService;

pub const SECRET: &str = "Gr9c-SeCrEt-7781";
const WAIT: Duration = Duration::from_secs(10);
const VERSION: &str = "9.9.9";

/// Call ids are UUIDs, as the webview makes them.
pub fn cid(n: u32) -> String {
    format!("00000000-0000-4000-8000-{n:012}")
}

pub fn row(name: &str, value: &str) -> KeyValue {
    KeyValue {
        name: name.into(),
        value: value.into(),
        enabled: true,
    }
}

/// A `kind: grpc` request on `test.v1.Echo/<method>` compiled from the protos folder.
pub fn content(url: &str, method: &str, message: &str) -> RequestContent {
    RequestContent {
        kind: RequestKind::Grpc,
        grpc: GrpcSettings {
            source: GrpcSource::Protos,
            protos: vec!["test/v1/echo.proto".into()],
            service: "test.v1.Echo".into(),
            method: method.into(),
            message: message.into(),
            ..GrpcSettings::default()
        },
        ..RequestContent::new("POST", url)
    }
}

/// The same request with its descriptors from server reflection.
pub fn reflected(url: &str, method: &str, message: &str) -> RequestContent {
    let mut request = content(url, method, message);
    request.grpc.source = GrpcSource::Reflection;
    request.grpc.protos.clear();
    request
}

fn secret_variable(name: &str, value: &str) -> VariableInput {
    VariableInput {
        name: name.into(),
        previous_name: None,
        value: String::new(),
        current: None,
        enabled: true,
        secret: true,
        secret_edit: SecretEdit::Set {
            value: value.into(),
        },
    }
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

pub fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub struct World {
    pub dir: tempfile::TempDir,
    pub http: HttpEngine,
    pub store: StoreState,
    pub vars: VarsService,
    pub state: GrpcState,
    pub location: RequestLocation,
    pub events: Arc<Mutex<Vec<GrpcEvent>>>,
}

pub fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let http = HttpEngine::new(&dir.path().join("cache"), dir.path(), VERSION).unwrap();
    let mut store = Store::new(&dir.path().join("data")).unwrap();
    let tree = store.create_collection(dir.path(), "C").unwrap();
    let vars = VarsService::new(Arc::new(MemoryVault::default()));
    let token = secret_variable("token", SECRET);
    vars.write_variables(&mut store, &VarScope::Global, &[token])
        .unwrap();
    let location = RequestLocation::Collection {
        collection_id: tree.collection.id,
        path: "r.http.yaml".into(),
    };
    let root = store.protos_root(&location).unwrap();
    copy_tree(&fixtures().join("grpc/server"), &root);
    copy_tree(&fixtures().join("grpc_files/protos"), &root);
    World {
        state: GrpcState::new(&dir.path().join("app-data")),
        dir,
        http,
        store: StoreState::new(store),
        vars,
        location,
        events: Arc::default(),
    }
}

impl World {
    pub fn deps(&self) -> GrpcDeps<'_> {
        GrpcDeps {
            http: &self.http,
            store: &self.store,
            vars: &self.vars,
            dynamics: Arc::new(SystemDynamics),
            state: &self.state,
            version: VERSION,
        }
    }

    pub fn protos_root(&self) -> PathBuf {
        let store = self.store.store.lock().unwrap();
        store.protos_root(&self.location).unwrap()
    }

    pub async fn schema(&self, request: &RequestContent) -> Result<GrpcSchema, AppError> {
        let found = self.deps().schema(&self.location, request, true).await?;
        Ok(found.expect("a schema"))
    }

    /// Without network: `None` when reflection has nothing cached.
    pub async fn schema_offline(
        &self,
        request: &RequestContent,
    ) -> Result<Option<GrpcSchema>, AppError> {
        self.deps().schema(&self.location, request, false).await
    }

    pub async fn reflect(
        &self,
        request: &RequestContent,
        refresh: bool,
    ) -> Result<GrpcSchema, AppError> {
        self.deps().reflect(&self.location, request, refresh).await
    }

    /// Starts `request` under `id`; events go through the real batching sink.
    pub async fn start(&self, id: &str, request: &RequestContent) -> Result<GrpcStarted, AppError> {
        let deps = self.deps();
        let (events, call_id) = (Arc::clone(&self.events), id.to_string());
        let sink_for = move |attempt: u32| -> Arc<dyn GrpcInbound> {
            Arc::new(GrpcBatchingSink::new(&call_id, attempt, move |event| {
                events.lock().unwrap().push(event);
            }))
        };
        let prepare = deps.prepare(&self.location, request);
        self.state.engine.start(id, prepare, sink_for).await
    }

    pub async fn send(&self, id: &str, message: &str) -> Result<WsMessage, AppError> {
        let deps = self.deps();
        let (json, secrets) = deps.resolve_message(&self.location, message).await?;
        self.state.engine.send(id, &json, &secrets).await
    }

    /// Every event of `id`, in emit order.
    pub fn events_of(&self, id: &str) -> Vec<GrpcEvent> {
        let events = self.events.lock().unwrap();
        events.iter().filter(|e| e.call_id == id).cloned().collect()
    }

    /// Waits for the `finished` of attempt `attempt` of `id` and returns its status.
    pub async fn finished(&self, id: &str, attempt: u32) -> Finish {
        let started = tokio::time::Instant::now();
        loop {
            let found = self
                .events_of(id)
                .into_iter()
                .find_map(|event| match event.kind {
                    GrpcEventKind::Finished {
                        status,
                        trailers,
                        by,
                        error,
                        ..
                    } if event.attempt == attempt => Some(Finish {
                        status,
                        trailers,
                        by: format!("{by:?}"),
                        error,
                    }),
                    _ => None,
                });
            if let Some(found) = found {
                return found;
            }
            assert!(
                started.elapsed() < WAIT,
                "no finished; saw {:?}",
                self.events_of(id)
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Received messages of `id` in its events (all attempts).
    pub fn received(&self, id: &str) -> Vec<String> {
        messages_of(&self.events_of(id), WsDirection::Received)
    }

    /// Waits until `id` has at least `count` received messages in its events.
    pub async fn until_received(&self, id: &str, count: usize) -> Vec<String> {
        let started = tokio::time::Instant::now();
        loop {
            let received = self.received(id);
            if received.len() >= count {
                return received;
            }
            assert!(started.elapsed() < WAIT, "saw {:?}", self.events_of(id));
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Waits until no call is starting or running.
    pub async fn idle(&self) {
        let started = tokio::time::Instant::now();
        while self.state.engine.live_count() > 0 {
            assert!(started.elapsed() < WAIT, "calls still live");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

pub fn messages_of(events: &[GrpcEvent], direction: WsDirection) -> Vec<String> {
    events
        .iter()
        .flat_map(|event| match &event.kind {
            GrpcEventKind::Messages { items } => items.clone(),
            _ => Vec::new(),
        })
        .filter(|item| item.direction == direction)
        .map(|item| item.text)
        .collect()
}

/// The `finished` event of a call, flattened for asserts.
#[derive(Debug, Clone)]
pub struct Finish {
    pub status: GrpcStatus,
    pub trailers: Vec<KeyValue>,
    /// `Client`, `Server` or `Error`.
    pub by: String,
    pub error: Option<AppError>,
}

impl Finish {
    pub fn trailer(&self, name: &str) -> Option<&str> {
        let found = self.trailers.iter().find(|row| row.name == name);
        found.map(|row| row.value.as_str())
    }
}
