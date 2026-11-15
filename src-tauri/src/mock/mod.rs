//! Mockoon-style mock server (plan 11).

pub mod commands;
pub mod disk;
pub mod engine;
pub mod matcher;
pub mod model;
pub mod request;
pub mod rules;
pub mod server;
#[cfg(test)]
mod service_tests;
pub mod template;
pub mod types;

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::error::AppError;
use model::MockEnvironment;
use server::{RunningServer, Shared};
use types::MockLogEntry;

/// Where served requests are announced: the Tauri event bridge in the app, nothing in tests.
pub trait MockSink: Send + Sync {
    fn logged(&self, collection_id: &str, file: &str, entry: &MockLogEntry);
}

struct Running {
    server: RunningServer,
    shared: Arc<Shared>,
}

fn key(collection_id: &str, file: &str) -> String {
    format!("{collection_id}/{file}")
}

/// All running mocks, keyed by collection and file.
pub struct MockService {
    sink: Arc<dyn MockSink>,
    running: Mutex<HashMap<String, Running>>,
}

impl MockService {
    pub fn new(sink: Arc<dyn MockSink>) -> Self {
        Self {
            sink,
            running: Mutex::new(HashMap::new()),
        }
    }

    /// Starts `mock`; returns the port it listens on.
    pub async fn start(
        &self,
        collection_id: &str,
        file: &str,
        mock: MockEnvironment,
    ) -> Result<u16, AppError> {
        let mut running = self.running.lock().await;
        let id = key(collection_id, file);
        if running.contains_key(&id) {
            return Err(AppError::invalid_input("That mock is already running."));
        }
        let shared = Arc::new(Shared::new(
            collection_id,
            file,
            mock,
            Arc::clone(&self.sink),
        ));
        let server = server::start(Arc::clone(&shared)).await?;
        let port = server.port;
        running.insert(id, Running { server, shared });
        Ok(port)
    }

    /// Stops the mock; stopping one that is not running is fine.
    pub async fn stop(&self, collection_id: &str, file: &str) {
        let removed = self.running.lock().await.remove(&key(collection_id, file));
        if let Some(entry) = removed {
            entry.server.stop().await;
        }
    }

    pub async fn stop_all(&self) {
        let all: Vec<Running> = self.running.lock().await.drain().map(|(_, r)| r).collect();
        for entry in all {
            entry.server.stop().await;
        }
    }

    /// The port of a running mock.
    pub async fn port_of(&self, collection_id: &str, file: &str) -> Option<u16> {
        self.running
            .lock()
            .await
            .get(&key(collection_id, file))
            .map(|r| r.server.port)
    }

    /// Newest last. Empty for a mock that is not running.
    pub async fn log(&self, collection_id: &str, file: &str) -> Vec<MockLogEntry> {
        match self.running.lock().await.get(&key(collection_id, file)) {
            Some(entry) => entry.shared.log().iter().cloned().collect(),
            None => Vec::new(),
        }
    }

    pub async fn clear_log(&self, collection_id: &str, file: &str) {
        if let Some(entry) = self.running.lock().await.get(&key(collection_id, file)) {
            entry.shared.log().clear();
        }
    }
}
