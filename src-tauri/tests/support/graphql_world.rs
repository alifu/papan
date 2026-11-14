//! Engine, store and variables for the GraphQL integration tests: global `base` (the
//! GraphQL server) and a secret `token` held in the vault only.

use std::sync::Arc;

use papan_lib::engine::graphql::service::GraphqlDeps;
use papan_lib::engine::graphql::SchemaCache;
use papan_lib::engine::http::scripted::{run_send, SendDeps};
use papan_lib::engine::http::types::SendOutcome;
use papan_lib::engine::http::HttpEngine;
use papan_lib::error::AppError;
use papan_lib::store::types::{RequestContent, RequestLocation};
use papan_lib::store::{Store, StoreState};
use papan_lib::vars::dynamic::SystemDynamics;
use papan_lib::vars::types::{SecretEdit, VarScope, VariableInput};
use papan_lib::vars::vault::MemoryVault;
use papan_lib::vars::VarsService;

pub struct World {
    pub dir: tempfile::TempDir,
    pub engine: HttpEngine,
    pub store: StoreState,
    pub vars: VarsService,
    pub cache: SchemaCache,
}

fn variable(name: &str, value: &str, secret: bool) -> VariableInput {
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

pub fn world(base: &str, token: &str) -> World {
    let dir = tempfile::tempdir().unwrap();
    let engine = HttpEngine::new(&dir.path().join("cache"), dir.path(), "9.9.9").unwrap();
    let mut store = Store::new(&dir.path().join("data")).unwrap();
    let vars = VarsService::new(Arc::new(MemoryVault::default()));
    vars.write_variables(
        &mut store,
        &VarScope::Global,
        &[
            variable("base", base, false),
            variable("token", token, true),
        ],
    )
    .unwrap();
    World {
        cache: SchemaCache::new(&dir.path().join("data")),
        dir,
        engine,
        store: StoreState::new(store),
        vars,
    }
}

pub fn scratch() -> RequestLocation {
    RequestLocation::Scratch { id: "g".into() }
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

impl World {
    pub fn graphql(&self) -> GraphqlDeps<'_> {
        GraphqlDeps {
            engine: &self.engine,
            store: &self.store,
            vars: &self.vars,
            dynamics: Arc::new(SystemDynamics),
            cache: &self.cache,
        }
    }

    pub async fn send(&self, id: &str, content: &RequestContent) -> Result<SendOutcome, AppError> {
        let deps = SendDeps {
            engine: &self.engine,
            store: &self.store,
            vars: &self.vars,
            dynamics: Arc::new(SystemDynamics),
        };
        run_send(&deps, id, &scratch(), content).await
    }

    pub fn history_len(&self) -> usize {
        self.store.lock_store().list_history().len()
    }
}
