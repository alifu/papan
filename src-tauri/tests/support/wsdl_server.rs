//! Local server serving WSDL/XSD documents for the SOAP import tests (plan 06 §11).
//!
//! - `GET /<path>`: a document added with [`Docs::add`] (404 otherwise);
//! - `/secure/...` paths need Basic `user:pass` (401 otherwise);
//! - `GET /redirect?to=<location>`: a 302 to `location`;
//! - every request is logged with its `Authorization` and `Cookie` headers.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::extract::{Query, Request, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use base64::Engine as _;

pub const USER: &str = "user";
pub const PASSWORD: &str = "pass";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: String,
    pub authorization: Option<String>,
    pub cookie: Option<String>,
}

/// The documents a server serves, by path (`/a.wsdl`).
#[derive(Debug, Clone, Default)]
pub struct Docs(HashMap<String, Vec<u8>>);

impl Docs {
    #[must_use]
    pub fn add(self, path: &str, body: impl Into<Vec<u8>>) -> Self {
        let mut docs = self.0;
        docs.insert(path.to_string(), body.into());
        Self(docs)
    }

    /// Adds a fixture of `tests/fixtures/wsdl` under `path`.
    #[must_use]
    pub fn fixture(self, path: &str, name: &str) -> Self {
        let file = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/wsdl")
            .join(name);
        let body = std::fs::read(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        self.add(path, body)
    }
}

#[derive(Clone)]
struct Shared {
    docs: Arc<Docs>,
    hits: Arc<Mutex<Vec<Hit>>>,
}

pub struct WsdlServer {
    pub addr: SocketAddr,
    hits: Arc<Mutex<Vec<Hit>>>,
}

impl WsdlServer {
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    pub fn hits(&self) -> Vec<Hit> {
        self.hits.lock().unwrap().clone()
    }
}

/// Starts a server for `docs` on `127.0.0.1:0` in the current tokio runtime.
pub async fn spawn(docs: Docs) -> WsdlServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(Mutex::new(Vec::new()));
    let shared = Shared {
        docs: Arc::new(docs),
        hits: Arc::clone(&hits),
    };
    let app = Router::new().fallback(serve).with_state(shared);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    WsdlServer { addr, hits }
}

fn header_text(request: &Request, name: header::HeaderName) -> Option<String> {
    request
        .headers()
        .get(name)
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
}

async fn serve(
    State(shared): State<Shared>,
    Query(query): Query<HashMap<String, String>>,
    request: Request,
) -> Response {
    let path = request.uri().path().to_string();
    let authorization = header_text(&request, header::AUTHORIZATION);
    shared.hits.lock().unwrap().push(Hit {
        path: path.clone(),
        authorization: authorization.clone(),
        cookie: header_text(&request, header::COOKIE),
    });
    if path == "/redirect" {
        let to = query.get("to").cloned().unwrap_or_default();
        return (StatusCode::FOUND, [(header::LOCATION, to)]).into_response();
    }
    if path.starts_with("/secure/") {
        let expected = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("{USER}:{PASSWORD}"))
        );
        if authorization.as_deref() != Some(expected.as_str()) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }
    match shared.docs.0.get(&path) {
        Some(body) => ([(header::CONTENT_TYPE, "text/xml")], body.clone()).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
