//! Harness for the webhook integration tests.

use std::sync::{Arc, Mutex};

use papan_lib::error::AppError;
use papan_lib::webhook::types::{
    CaptureSummary, WebhookBind, WebhookConfig, WebhookListener, WebhookResponse,
};
use papan_lib::webhook::{CaptureSink, WebhookService};

#[derive(Default)]
pub struct Events(
    pub Mutex<Vec<(String, CaptureSummary)>>,
    pub Mutex<Vec<(String, Option<AppError>)>>,
);

impl CaptureSink for Events {
    fn captured(&self, listener_id: &str, capture: &CaptureSummary) {
        if let Ok(mut all) = self.0.lock() {
            all.push((listener_id.to_string(), capture.clone()));
        }
    }

    fn stopped(&self, listener_id: &str, error: Option<&AppError>) {
        if let Ok(mut all) = self.1.lock() {
            all.push((listener_id.to_string(), error.cloned()));
        }
    }
}

pub struct World {
    pub dir: tempfile::TempDir,
    pub service: WebhookService,
    pub events: Arc<Events>,
}

pub fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(Events::default());
    let service = WebhookService::new(dir.path(), events.clone());
    World {
        dir,
        service,
        events,
    }
}

pub fn config(path: &str) -> WebhookConfig {
    WebhookConfig {
        id: String::new(),
        name: "Test".into(),
        bind: WebhookBind::Loopback,
        port: 0,
        path: path.into(),
        response: WebhookResponse {
            status: 200,
            headers: vec![],
            body: String::new(),
        },
        max_body_bytes: 0,
        max_captures: 0,
    }
}

pub async fn running(world: &World, config: WebhookConfig) -> WebhookListener {
    let saved = world.service.save(config).await.unwrap();
    world.service.start(&saved.config.id).await.unwrap()
}

pub fn url_of(listener: &WebhookListener) -> String {
    listener.url.clone().unwrap()
}
