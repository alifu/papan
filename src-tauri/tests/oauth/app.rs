//! The fixture of `tests/oauth.rs`: an engine over a file vault, fake browsers and a
//! global log capture.

use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};

use papan_lib::engine::http::oauth::flow::token_for_send;
use papan_lib::engine::http::oauth::{config_key, BrowserOpener, OAuth, OAuthConfig};
use papan_lib::engine::http::resolve::Resolved;
use papan_lib::engine::http::types::{OAuthStatus, ResponseMeta};
use papan_lib::engine::http::HttpEngine;
use papan_lib::error::AppError;
use papan_lib::store::types::{Auth, ClientAuth, OAuthGrant, RequestContent, RequestLocation};
use papan_lib::vars::vault::{FileVault, MemoryKeys, SecretStore};
use url::Url;

use crate::support::idp::{TestIdp, CLIENT_ID, CLIENT_SECRET};

pub const COLLECTION: &str = "7f1c2a34-5b6d-4e8f-9a0b-1c2d3e4f5a6b";
pub const OTHER_COLLECTION: &str = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";

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

static LOGS: OnceLock<Capture> = OnceLock::new();

/// Starts capturing every log line of this test binary (all levels).
pub fn capture_logs() {
    LOGS.get_or_init(|| {
        let capture = Capture::default();
        let writer = capture.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .finish();
        tracing::subscriber::set_global_default(subscriber).unwrap();
        capture
    });
}

pub fn logs() -> String {
    let capture = LOGS.get().expect("capture_logs() first");
    String::from_utf8_lossy(&capture.0.lock().unwrap()).into_owned()
}

/// A browser that follows the provider's redirect back to the listener.
#[derive(Default)]
pub struct FakeBrowser {
    pub opened: Mutex<Vec<Url>>,
}

impl BrowserOpener for FakeBrowser {
    fn open(&self, url: &Url) -> Result<(), AppError> {
        self.opened.lock().unwrap().push(url.clone());
        let url = url.clone();
        tokio::spawn(async move {
            let _ = reqwest::get(url).await;
        });
        Ok(())
    }
}

/// A browser that only hands the URL to the test.
pub struct ManualBrowser(pub tokio::sync::mpsc::UnboundedSender<Url>);

impl BrowserOpener for ManualBrowser {
    fn open(&self, url: &Url) -> Result<(), AppError> {
        self.0.send(url.clone()).unwrap();
        Ok(())
    }
}

pub fn query(url: &Url, name: &str) -> String {
    url.query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
        .unwrap_or_default()
}

pub fn collection(id: &str) -> RequestLocation {
    RequestLocation::Collection {
        collection_id: id.to_string(),
        path: "request.yaml".to_string(),
    }
}

/// A draft for the provider's `/resource` with its OAuth 2 section.
pub fn draft(idp: &TestIdp, grant: OAuthGrant) -> Resolved {
    draft_with(idp, grant, |_| {})
}

pub fn draft_with(idp: &TestIdp, grant: OAuthGrant, change: impl FnOnce(&mut Auth)) -> Resolved {
    let mut auth = Auth::Oauth2 {
        grant,
        auth_url: idp.authorize_url(),
        token_url: idp.token_url(),
        client_id: CLIENT_ID.to_string(),
        client_secret: CLIENT_SECRET.to_string(),
        scope: "read write".to_string(),
        client_auth: ClientAuth::Basic,
        redirect_port: 0,
    };
    change(&mut auth);
    Resolved::unchanged(RequestContent {
        auth,
        ..RequestContent::new("GET", idp.url("/resource"))
    })
}

pub fn key_of(resolved: &Resolved) -> String {
    config_key(&OAuthConfig::from_resolved(resolved).unwrap().unwrap())
}

pub struct App {
    pub dir: tempfile::TempDir,
    pub vault: Arc<dyn SecretStore>,
    pub engine: HttpEngine,
    pub location: RequestLocation,
}

impl App {
    pub fn new() -> Self {
        capture_logs();
        let dir = tempfile::tempdir().unwrap();
        let vault: Arc<dyn SecretStore> = Arc::new(FileVault::new(
            dir.path().join("secrets.vault"),
            MemoryKeys::new(),
        ));
        let engine = HttpEngine::new(&dir.path().join("cache"), dir.path(), "9.9.9")
            .unwrap()
            .with_vault(dir.path(), Arc::clone(&vault));
        Self {
            dir,
            vault,
            engine,
            location: collection(COLLECTION),
        }
    }

    pub fn oauth(&self) -> &OAuth {
        self.engine.oauth()
    }

    pub async fn authorize(
        &self,
        resolved: &Resolved,
        browser: &dyn BrowserOpener,
    ) -> Result<OAuthStatus, AppError> {
        let flow_id = uuid::Uuid::new_v4().to_string();
        self.oauth()
            .authorize(
                &flow_id,
                &self.location,
                resolved,
                &key_of(resolved),
                browser,
            )
            .await
    }

    /// What the send hook does: the stored token attached, or the refusal.
    pub async fn attach(&self, resolved: &Resolved) -> Result<Resolved, AppError> {
        self.attach_at(&self.location, resolved).await
    }

    pub async fn attach_at(
        &self,
        location: &RequestLocation,
        resolved: &Resolved,
    ) -> Result<Resolved, AppError> {
        let net = self.engine.network().current().unwrap();
        token_for_send(self.oauth(), &net, location, resolved).await
    }

    /// Sends the draft through the engine, whose send hook attaches the token.
    pub async fn send(&self, resolved: &Resolved) -> Result<ResponseMeta, AppError> {
        let id = uuid::Uuid::new_v4().to_string();
        self.engine
            .send_resolved(&id, &self.location, resolved)
            .await
    }

    /// Every file under the app data directory, as bytes.
    pub fn files(&self) -> Vec<(String, Vec<u8>)> {
        let mut found = Vec::new();
        let mut pending = vec![self.dir.path().to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    found.push((path.display().to_string(), std::fs::read(&path).unwrap()));
                }
            }
        }
        found
    }
}
