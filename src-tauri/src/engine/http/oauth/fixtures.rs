//! Test fixtures for the OAuth 2 unit tests: a draft with an OAuth 2 section and a
//! scripted token endpoint with a hit log.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use serde_json::{json, Value};

use super::super::client::Clients;
use super::super::network::Network;
use super::super::resolve::Resolved;
use super::{BrowserOpener, OAuth, OAuthConfig};
use crate::error::AppError;
use crate::store::types::{Auth, ClientAuth, OAuthGrant, RequestContent, RequestLocation};
use crate::vars::vault::{MemoryVault, SecretStore};

pub const CLIENT_ID: &str = "unit-client";
pub const CLIENT_SECRET: &str = "unit-client-secret-51c2";
pub const COLLECTION: &str = "7f1c2a34-5b6d-4e8f-9a0b-1c2d3e4f5a6b";
pub const OTHER_COLLECTION: &str = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";

pub fn collection(id: &str) -> RequestLocation {
    RequestLocation::Collection {
        collection_id: id.to_string(),
        path: "request.yaml".to_string(),
    }
}

pub fn oauth2(grant: OAuthGrant, auth_url: &str, token_url: &str) -> Auth {
    Auth::Oauth2 {
        grant,
        auth_url: auth_url.to_string(),
        token_url: token_url.to_string(),
        client_id: CLIENT_ID.to_string(),
        client_secret: CLIENT_SECRET.to_string(),
        scope: "read write".to_string(),
        client_auth: ClientAuth::Basic,
        redirect_port: 0,
    }
}

pub fn resolved_with(auth: Auth) -> Resolved {
    Resolved::unchanged(RequestContent {
        auth,
        ..RequestContent::new("GET", "https://api.example.test/items")
    })
}

pub fn resolved(grant: OAuthGrant, auth_url: &str, token_url: &str) -> Resolved {
    resolved_with(oauth2(grant, auth_url, token_url))
}

pub fn config(grant: OAuthGrant, auth_url: &str, token_url: &str) -> OAuthConfig {
    OAuthConfig::from_resolved(&resolved(grant, auth_url, token_url))
        .unwrap()
        .unwrap()
}

/// An `OAuth` over a fresh memory vault, with that vault.
pub fn oauth() -> (OAuth, Arc<dyn SecretStore>) {
    let dir = tempfile::tempdir().unwrap();
    let vault: Arc<dyn SecretStore> = Arc::new(MemoryVault::default());
    let network = Arc::new(Network::load(dir.path(), Arc::clone(&vault)));
    (OAuth::new(Arc::new(Clients::default()), network), vault)
}

/// An opener that must never be reached; it records every call.
#[derive(Debug, Default)]
pub struct NoBrowser(pub Mutex<Vec<String>>);

impl BrowserOpener for NoBrowser {
    fn open(&self, url: &url::Url) -> Result<(), AppError> {
        self.0.lock().unwrap().push(url.to_string());
        Err(AppError::internal("no browser in this test"))
    }
}

/// One request the endpoint received.
#[derive(Debug, Clone)]
pub struct Hit {
    pub form: Vec<(String, String)>,
    pub authorization: Option<String>,
    pub accept: Option<String>,
    pub cookie: Option<String>,
}

impl Hit {
    pub fn param(&self, name: &str) -> Option<&str> {
        self.form
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Default)]
struct Script {
    replies: Mutex<VecDeque<(u16, String)>>,
    hits: Mutex<Vec<Hit>>,
    delay: Mutex<Duration>,
}

/// A token endpoint that answers with queued replies (then `500`).
pub struct Endpoint {
    pub addr: SocketAddr,
    script: Arc<Script>,
}

impl Endpoint {
    pub fn url(&self) -> String {
        format!("http://{}/token", self.addr)
    }

    pub fn reply(&self, status: u16, body: &Value) {
        let mut replies = self.script.replies.lock().unwrap();
        replies.push_back((status, body.to_string()));
    }

    /// Every reply waits this long first.
    pub fn delay(&self, delay: Duration) {
        *self.script.delay.lock().unwrap() = delay;
    }

    pub fn hits(&self) -> Vec<Hit> {
        self.script.hits.lock().unwrap().clone()
    }
}

/// A bearer token response.
pub fn bearer(access: &str, refresh: Option<&str>, expires_in: Option<u64>) -> Value {
    let mut body = json!({ "access_token": access, "token_type": "Bearer" });
    if let Some(refresh) = refresh {
        body["refresh_token"] = json!(refresh);
    }
    if let Some(expires_in) = expires_in {
        body["expires_in"] = json!(expires_in);
    }
    body
}

fn text(headers: &HeaderMap, name: header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

async fn token(State(script): State<Arc<Script>>, headers: HeaderMap, body: Bytes) -> Response {
    script.hits.lock().unwrap().push(Hit {
        form: url::form_urlencoded::parse(&body).into_owned().collect(),
        authorization: text(&headers, header::AUTHORIZATION),
        accept: text(&headers, header::ACCEPT),
        cookie: text(&headers, header::COOKIE),
    });
    let delay = *script.delay.lock().unwrap();
    tokio::time::sleep(delay).await;
    let next = script.replies.lock().unwrap().pop_front();
    let (status, body) = next.unwrap_or((500, "unscripted".to_string()));
    let status = StatusCode::from_u16(status).unwrap();
    let headers = [
        (header::CONTENT_TYPE, "application/json"),
        (header::SET_COOKIE, "session=endpoint-cookie; Path=/"),
        (header::LOCATION, "/elsewhere"),
    ];
    (status, headers, body).into_response()
}

pub async fn endpoint() -> Endpoint {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let script = Arc::new(Script::default());
    let app = Router::new()
        .route("/token", post(token))
        .route("/elsewhere", post(token))
        .with_state(Arc::clone(&script));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Endpoint { addr, script }
}
