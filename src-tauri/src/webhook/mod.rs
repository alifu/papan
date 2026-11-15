//! Webhook listener that captures incoming requests (plan 07 §6).

pub mod capture;
pub mod commands;
pub mod config;
pub mod replay;
pub mod respond;
pub mod server;
pub mod types;

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::error::AppError;
use crate::store::scratch::Scratch;
use crate::store::types::ScratchItem;
use config::ConfigStore;
use server::{RunningServer, Shared};
use types::{Capture, CaptureSummary, ReplayResult, WebhookBind, WebhookConfig, WebhookListener};

#[cfg(test)]
pub(crate) mod fixtures;

/// Where captures are announced: the Tauri event bridge in the app, a channel in tests.
pub trait CaptureSink: Send + Sync {
    fn captured(&self, listener_id: &str, capture: &CaptureSummary);
    /// A listener stopped on its own, with the reason when it failed.
    fn stopped(&self, listener_id: &str, error: Option<&AppError>);
}

struct Entry {
    config: WebhookConfig,
    shared: Arc<Shared>,
    running: Option<RunningServer>,
    /// Bumped by every save, so a start that began before it can tell its config is stale.
    generation: u64,
}

/// What became of a server a start has built.
enum Install {
    Done,
    /// Not installed: `stale` when a save changed the config meanwhile.
    Rejected {
        server: RunningServer,
        stale: bool,
    },
}

struct State {
    entries: Vec<Entry>,
}

/// All listeners: configs, running servers and their captures.
pub struct WebhookService {
    configs: ConfigStore,
    sink: Arc<dyn CaptureSink>,
    state: Mutex<State>,
}

fn unknown() -> AppError {
    AppError::not_found("That webhook listener does not exist.")
}

impl WebhookService {
    pub fn new(app_data: &Path, sink: Arc<dyn CaptureSink>) -> Self {
        let configs = ConfigStore::new(app_data);
        let entries = configs
            .load()
            .into_iter()
            .map(|config| Entry {
                shared: Arc::new(Shared::new(
                    &config.id,
                    config.max_captures,
                    Arc::clone(&sink),
                )),
                config,
                running: None,
                generation: 0,
            })
            .collect();
        Self {
            configs,
            sink,
            state: Mutex::new(State { entries }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn list(&self) -> Vec<WebhookListener> {
        self.lock().entries.iter().map(view).collect()
    }

    /// Creates (empty `id`) or updates a listener; a running one restarts with the new config.
    pub async fn save(&self, config: WebhookConfig) -> Result<WebhookListener, AppError> {
        let mut config = config::normalize(config);
        if config.id.is_empty() {
            config.id = uuid::Uuid::new_v4().to_string();
        }
        config::validate(&config)?;
        let (was_running, stale) = {
            let mut state = self.lock();
            let known = state.entries.iter().position(|e| e.config.id == config.id);
            match known {
                Some(at) => {
                    let entry = &mut state.entries[at];
                    entry.config = config.clone();
                    entry.generation += 1;
                    let was_running = entry.running.is_some();
                    entry
                        .shared
                        .ring()
                        .set_max_items(config.max_captures as usize);
                    (was_running, entry.running.take())
                }
                None => {
                    if state.entries.len() >= config::MAX_LISTENERS {
                        return Err(AppError::invalid_input(
                            "At most 20 webhook listeners are allowed.",
                        ));
                    }
                    let shared = Arc::new(Shared::new(
                        &config.id,
                        config.max_captures,
                        Arc::clone(&self.sink),
                    ));
                    state.entries.push(Entry {
                        config: config.clone(),
                        shared,
                        running: None,
                        generation: 0,
                    });
                    (false, None)
                }
            }
        };
        if let Some(server) = stale {
            server.stop().await;
        }
        self.persist()?;
        if was_running {
            return match self.start(&config.id).await {
                Ok(listener) => Ok(listener),
                Err(error) => {
                    self.sink.stopped(&config.id, Some(&error));
                    Err(error)
                }
            };
        }
        self.listener(&config.id)
    }

    pub async fn delete(&self, id: &str) -> Result<(), AppError> {
        let removed = {
            let mut state = self.lock();
            let at = state
                .entries
                .iter()
                .position(|e| e.config.id == id)
                .ok_or_else(unknown)?;
            state.entries.remove(at)
        };
        if let Some(server) = removed.running {
            server.stop().await;
        }
        self.persist()
    }

    pub async fn start(&self, id: &str) -> Result<WebhookListener, AppError> {
        loop {
            let (config, shared, generation) = {
                let state = self.lock();
                let entry = state
                    .entries
                    .iter()
                    .find(|e| e.config.id == id)
                    .ok_or_else(unknown)?;
                if entry.running.is_some() {
                    return Ok(view(entry));
                }
                let taken = entry.config.port != 0
                    && state.entries.iter().any(|other| {
                        other.config.id != id
                            && other
                                .running
                                .as_ref()
                                .is_some_and(|server| u32::from(server.port) == entry.config.port)
                    });
                if taken {
                    return Err(AppError::invalid_input(format!(
                        "Port {} is already used by another Papan listener.",
                        entry.config.port
                    )));
                }
                (
                    entry.config.clone(),
                    Arc::clone(&entry.shared),
                    entry.generation,
                )
            };
            let server = server::start(&config, shared).await?;
            match self.install(id, generation, server) {
                Install::Done => return self.listener(id),
                Install::Rejected { server, stale } => {
                    server.stop().await;
                    if !stale {
                        return self.listener(id);
                    }
                }
            }
        }
    }

    /// Puts `server`, built from the config of `generation`, into service unless the
    /// listener is gone, already running, or was saved since.
    fn install(&self, id: &str, generation: u64, server: RunningServer) -> Install {
        let mut state = self.lock();
        match state.entries.iter_mut().find(|e| e.config.id == id) {
            Some(entry) if entry.generation != generation => Install::Rejected {
                server,
                stale: true,
            },
            Some(entry) if entry.running.is_none() => {
                entry.running = Some(server);
                Install::Done
            }
            _ => Install::Rejected {
                server,
                stale: false,
            },
        }
    }

    pub async fn stop(&self, id: &str) -> Result<(), AppError> {
        let server = {
            let mut state = self.lock();
            state
                .entries
                .iter_mut()
                .find(|e| e.config.id == id)
                .ok_or_else(unknown)?
                .running
                .take()
        };
        if let Some(server) = server {
            server.stop().await;
        }
        Ok(())
    }

    /// Stops every running listener (app exit).
    pub async fn stop_all(&self) {
        let servers: Vec<RunningServer> = self
            .lock()
            .entries
            .iter_mut()
            .filter_map(|e| e.running.take())
            .collect();
        for server in servers {
            server.stop().await;
        }
    }

    pub fn captures(&self, id: &str) -> Result<Vec<CaptureSummary>, AppError> {
        Ok(self.shared(id)?.ring().summaries())
    }

    pub fn read_capture(&self, id: &str, capture_id: &str) -> Result<Capture, AppError> {
        self.shared(id)?
            .ring()
            .find(capture_id)
            .ok_or_else(|| AppError::not_found("That capture is no longer available."))
    }

    pub fn clear_captures(&self, id: &str) -> Result<(), AppError> {
        let shared = self.shared(id)?;
        shared.ring().clear();
        shared.unmatched.store(0, Ordering::Relaxed);
        Ok(())
    }

    /// A new scratch request from a capture (the caller supplies the scratch store).
    pub fn replay(
        &self,
        id: &str,
        capture_id: &str,
        scratch: &Scratch,
    ) -> Result<ReplayResult, AppError> {
        let capture = self.read_capture(id, capture_id)?;
        let listener = self.listener(id)?;
        let (content, warnings) = replay::capture_to_content(&capture, &listener);
        let item: ScratchItem = scratch.create_from(&replay::replay_name(&capture), &content)?;
        Ok(ReplayResult { item, warnings })
    }

    fn shared(&self, id: &str) -> Result<Arc<Shared>, AppError> {
        let state = self.lock();
        let entry = state
            .entries
            .iter()
            .find(|e| e.config.id == id)
            .ok_or_else(unknown)?;
        Ok(Arc::clone(&entry.shared))
    }

    fn listener(&self, id: &str) -> Result<WebhookListener, AppError> {
        let state = self.lock();
        state
            .entries
            .iter()
            .find(|e| e.config.id == id)
            .map(view)
            .ok_or_else(unknown)
    }

    fn persist(&self) -> Result<(), AppError> {
        let configs: Vec<WebhookConfig> = self
            .lock()
            .entries
            .iter()
            .map(|e| e.config.clone())
            .collect();
        self.configs.save(&configs)
    }
}

fn view(entry: &Entry) -> WebhookListener {
    let url = entry.running.as_ref().map(|server| {
        let host = match entry.config.bind {
            WebhookBind::Loopback => "127.0.0.1",
            WebhookBind::AllInterfaces => "0.0.0.0",
        };
        format!("http://{host}:{}{}", server.port, entry.config.path)
    });
    WebhookListener {
        config: entry.config.clone(),
        running: entry.running.is_some(),
        url,
        captured: u32::try_from(entry.shared.ring().len()).unwrap_or(u32::MAX),
        unmatched: entry.shared.unmatched.load(Ordering::Relaxed),
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::sample;
    use super::*;

    struct Quiet;

    impl CaptureSink for Quiet {
        fn captured(&self, _: &str, _: &CaptureSummary) {}
        fn stopped(&self, _: &str, _: Option<&AppError>) {}
    }

    #[tokio::test]
    async fn a_server_built_before_a_save_is_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let service = WebhookService::new(dir.path(), Arc::new(Quiet));
        let saved = service.save(sample()).await.unwrap();
        let id = saved.config.id.clone();
        let (config, shared, generation) = {
            let state = service.lock();
            let entry = &state.entries[0];
            (
                entry.config.clone(),
                Arc::clone(&entry.shared),
                entry.generation,
            )
        };
        let old = server::start(&config, shared).await.unwrap();
        let mut edited = saved.config;
        edited.path = "/edited".into();
        service.save(edited).await.unwrap();

        let outcome = service.install(&id, generation, old);

        let Install::Rejected { server, stale } = outcome else {
            panic!("a stale server must be rejected");
        };
        assert!(stale);
        server.stop().await;
        assert!(!service.list()[0].running);
    }
}
