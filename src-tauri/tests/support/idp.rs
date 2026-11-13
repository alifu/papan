//! A test OAuth 2 provider (plan 03b §10, phase B). `GET /authorize` redirects (`302`) to
//! the `redirect_uri` with `code` and `state`, or `error`. `POST /token` serves
//! `authorization_code` (PKCE S256, single-use codes), `client_credentials` and
//! `refresh_token` with `basic` or `body` client auth; errors are RFC 6749 §5.2 JSON.
//! `/token-redirect` answers `307` to `/token` (must not be followed); `GET /resource`
//! accepts only a live bearer token. Every request, body included, is in the hit log.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use base64::Engine as _;
use serde_json::{json, Map, Value};

pub const CLIENT_ID: &str = "papan-test-client";
pub const CLIENT_SECRET: &str = "idp-client-secret-7f3a9c";
const DEFAULT_LIFETIME: Duration = Duration::from_secs(3600);
const MAX_BODY: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientAuth {
    /// `Authorization: Basic base64(client_id:client_secret)`.
    Basic,
    /// `client_id` and `client_secret` in the form body.
    Body,
}

#[derive(Debug, Clone)]
pub struct IdpConfig {
    pub client_id: String,
    pub client_secret: String,
    pub client_auth: ClientAuth,
    /// `expires_in` of issued access tokens; `None` omits it (the token never expires).
    pub token_lifetime: Option<Duration>,
    /// On refresh, issue a new refresh token and revoke the old one; otherwise the
    /// response carries no `refresh_token` and the old one stays valid.
    pub rotate_refresh: bool,
    /// `token_type` of every token response (`"mac"` to test refusal).
    pub token_type: String,
    /// `/authorize` redirects with this `error` instead of a code.
    pub authorize_error: Option<String>,
    /// `/token` answers `400` with this `error` to every grant.
    pub token_error: Option<String>,
}

impl Default for IdpConfig {
    fn default() -> Self {
        Self {
            client_id: CLIENT_ID.to_string(),
            client_secret: CLIENT_SECRET.to_string(),
            client_auth: ClientAuth::Basic,
            token_lifetime: Some(DEFAULT_LIFETIME),
            rotate_refresh: true,
            token_type: "Bearer".to_string(),
            authorize_error: None,
            token_error: None,
        }
    }
}

/// One request the provider received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub form: Vec<(String, String)>,
    pub authorization: Option<String>,
    pub host: Option<String>,
    pub cookie: Option<String>,
}

impl Hit {
    /// A form field, then a query parameter, by name.
    pub fn param(&self, name: &str) -> Option<&str> {
        find(&self.form, name).or_else(|| find(&self.query, name))
    }
}

struct Pending {
    challenge: String,
    redirect_uri: String,
    scope: String,
}

#[derive(Default)]
struct Ledger {
    codes: HashMap<String, Pending>,
    /// Refresh token → scope.
    refresh: HashMap<String, String>,
    /// Access token → expiry (`None` = never).
    access: HashMap<String, Option<Instant>>,
    /// Every code and token ever issued, for leak checks.
    issued: Vec<String>,
}

struct Shared {
    config: Mutex<IdpConfig>,
    ledger: Mutex<Ledger>,
    hits: Mutex<Vec<Hit>>,
}

pub struct TestIdp {
    pub addr: SocketAddr,
    shared: Arc<Shared>,
}

impl TestIdp {
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    pub fn authorize_url(&self) -> String {
        self.url("/authorize")
    }

    pub fn token_url(&self) -> String {
        self.url("/token")
    }

    pub fn hits(&self) -> Vec<Hit> {
        self.shared.hits.lock().unwrap().clone()
    }

    /// Requests to `/token` (and `/token-redirect`).
    pub fn token_hits(&self) -> Vec<Hit> {
        self.hits()
            .into_iter()
            .filter(|hit| hit.path.starts_with("/token"))
            .collect()
    }

    /// Every authorization code, access token and refresh token issued so far.
    pub fn issued(&self) -> Vec<String> {
        self.shared.ledger.lock().unwrap().issued.clone()
    }

    /// Changes the configuration of the running provider.
    pub fn configure(&self, change: impl FnOnce(&mut IdpConfig)) {
        change(&mut self.shared.config.lock().unwrap());
    }

    /// Makes every access token issued so far expired (the refresh tokens stay valid).
    pub fn expire_access_tokens(&self) {
        let past = Some(Instant::now() - Duration::from_secs(1));
        let mut ledger = self.shared.ledger.lock().unwrap();
        ledger.access.values_mut().for_each(|expiry| *expiry = past);
    }
}

/// Starts the provider on `127.0.0.1:0` in the current tokio runtime.
pub async fn spawn(config: IdpConfig) -> TestIdp {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let shared = Arc::new(Shared {
        config: Mutex::new(config),
        ledger: Mutex::new(Ledger::default()),
        hits: Mutex::new(Vec::new()),
    });
    let app = Router::new()
        .route("/authorize", get(authorize))
        .route("/token", post(token))
        .route("/token-redirect", any(token_redirect))
        .route("/resource", get(resource))
        .layer(middleware::from_fn_with_state(Arc::clone(&shared), log_hit))
        .with_state(Arc::clone(&shared));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    TestIdp { addr, shared }
}

/// base64url(SHA-256(verifier)) without padding (RFC 7636 §4.2).
pub fn s256(verifier: &str) -> String {
    let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest.as_ref())
}

fn find<'a>(pairs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn pairs(bytes: &[u8]) -> Vec<(String, String)> {
    url::form_urlencoded::parse(bytes).into_owned().collect()
}

fn header_text(headers: &HeaderMap, name: header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn record(shared: &Shared, parts: &Parts, body: &[u8]) {
    shared.hits.lock().unwrap().push(Hit {
        method: parts.method.to_string(),
        path: parts.uri.path().to_string(),
        query: pairs(parts.uri.query().unwrap_or_default().as_bytes()),
        form: pairs(body),
        authorization: header_text(&parts.headers, header::AUTHORIZATION),
        host: header_text(&parts.headers, header::HOST),
        cookie: header_text(&parts.headers, header::COOKIE),
    });
}

/// Logs every request, body included, before routing.
async fn log_hit(State(shared): State<Arc<Shared>>, request: Request, next: Next) -> Response {
    let (parts, body) = request.into_parts();
    let bytes = axum::body::to_bytes(body, MAX_BODY)
        .await
        .unwrap_or_default();
    record(&shared, &parts, &bytes);
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

fn new_secret(shared: &Shared, kind: &str) -> String {
    let value = format!("{kind}-{}", uuid::Uuid::new_v4().simple());
    shared.ledger.lock().unwrap().issued.push(value.clone());
    value
}

fn oauth_error(status: StatusCode, error: &str) -> Response {
    let body = json!({ "error": error, "error_description": format!("Test provider: {error}.") });
    (status, Json(body)).into_response()
}

fn redirect(location: &str, status: StatusCode) -> Response {
    (status, [(header::LOCATION, location.to_string())]).into_response()
}

async fn authorize(State(shared): State<Arc<Shared>>, uri: Uri) -> Response {
    let query = pairs(uri.query().unwrap_or_default().as_bytes());
    let get = |name: &str| find(&query, name).unwrap_or_default().to_string();
    let Ok(mut target) = url::Url::parse(&get("redirect_uri")) else {
        return (StatusCode::BAD_REQUEST, "redirect_uri is missing").into_response();
    };
    let config = shared.config.lock().unwrap().clone();
    let well_formed = get("response_type") == "code"
        && get("client_id") == config.client_id
        && get("code_challenge_method") == "S256"
        && !get("code_challenge").is_empty();
    let error = config
        .authorize_error
        .or_else(|| (!well_formed).then(|| "invalid_request".to_string()));
    match error {
        Some(error) => {
            target
                .query_pairs_mut()
                .append_pair("error", &error)
                .append_pair("error_description", "Test provider refused.");
        }
        None => {
            let code = new_secret(&shared, "code");
            let pending = Pending {
                challenge: get("code_challenge"),
                redirect_uri: get("redirect_uri"),
                scope: get("scope"),
            };
            shared
                .ledger
                .lock()
                .unwrap()
                .codes
                .insert(code.clone(), pending);
            target.query_pairs_mut().append_pair("code", &code);
        }
    }
    target.query_pairs_mut().append_pair("state", &get("state"));
    redirect(target.as_str(), StatusCode::FOUND)
}

fn client_authenticated(
    config: &IdpConfig,
    headers: &HeaderMap,
    form: &[(String, String)],
) -> bool {
    match config.client_auth {
        ClientAuth::Basic => {
            let pair = format!("{}:{}", config.client_id, config.client_secret);
            let expected = format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(pair)
            );
            header_text(headers, header::AUTHORIZATION).as_deref() == Some(expected.as_str())
        }
        ClientAuth::Body => {
            find(form, "client_id") == Some(config.client_id.as_str())
                && find(form, "client_secret") == Some(config.client_secret.as_str())
        }
    }
}

/// A token response; `refresh` adds a new refresh token for `scope`.
fn issue(shared: &Shared, config: &IdpConfig, scope: &str, refresh: bool) -> Response {
    let access = new_secret(shared, "access");
    let expiry = config
        .token_lifetime
        .map(|lifetime| Instant::now() + lifetime);
    let mut body = Map::new();
    body.insert("access_token".into(), Value::from(access.as_str()));
    body.insert("token_type".into(), Value::from(config.token_type.as_str()));
    body.insert("scope".into(), Value::from(scope));
    if let Some(lifetime) = config.token_lifetime {
        body.insert("expires_in".into(), Value::from(lifetime.as_secs()));
    }
    let refresh_token = refresh.then(|| new_secret(shared, "refresh"));
    if let Some(token) = &refresh_token {
        body.insert("refresh_token".into(), Value::from(token.as_str()));
    }
    let mut ledger = shared.ledger.lock().unwrap();
    ledger.access.insert(access, expiry);
    if let Some(token) = refresh_token {
        ledger.refresh.insert(token, scope.to_string());
    }
    Json(Value::Object(body)).into_response()
}

fn authorization_code(shared: &Shared, config: &IdpConfig, form: &[(String, String)]) -> Response {
    let code = find(form, "code").unwrap_or_default();
    let Some(pending) = shared.ledger.lock().unwrap().codes.remove(code) else {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant");
    };
    let verifier = find(form, "code_verifier").unwrap_or_default();
    let redirect_uri = find(form, "redirect_uri").unwrap_or_default();
    if redirect_uri != pending.redirect_uri || s256(verifier) != pending.challenge {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant");
    }
    issue(shared, config, &pending.scope, true)
}

fn refresh_token(shared: &Shared, config: &IdpConfig, form: &[(String, String)]) -> Response {
    let token = find(form, "refresh_token").unwrap_or_default();
    let scope = {
        let mut ledger = shared.ledger.lock().unwrap();
        let scope = ledger.refresh.get(token).cloned();
        if config.rotate_refresh {
            ledger.refresh.remove(token);
        }
        scope
    };
    match scope {
        Some(scope) => issue(shared, config, &scope, config.rotate_refresh),
        None => oauth_error(StatusCode::BAD_REQUEST, "invalid_grant"),
    }
}

async fn token(State(shared): State<Arc<Shared>>, headers: HeaderMap, body: Bytes) -> Response {
    let form = pairs(&body);
    let config = shared.config.lock().unwrap().clone();
    if let Some(error) = &config.token_error {
        return oauth_error(StatusCode::BAD_REQUEST, error);
    }
    if !client_authenticated(&config, &headers, &form) {
        return oauth_error(StatusCode::UNAUTHORIZED, "invalid_client");
    }
    match find(&form, "grant_type").unwrap_or_default() {
        "authorization_code" => authorization_code(&shared, &config, &form),
        "client_credentials" => {
            let scope = find(&form, "scope").unwrap_or_default();
            issue(&shared, &config, scope, false)
        }
        "refresh_token" => refresh_token(&shared, &config, &form),
        _ => oauth_error(StatusCode::BAD_REQUEST, "unsupported_grant_type"),
    }
}

async fn token_redirect() -> Response {
    redirect("/token", StatusCode::TEMPORARY_REDIRECT)
}

async fn resource(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    let presented = header_text(&headers, header::AUTHORIZATION)
        .and_then(|value| value.strip_prefix("Bearer ").map(str::to_string));
    let expiry =
        presented.and_then(|token| shared.ledger.lock().unwrap().access.get(&token).copied());
    let live = expiry.is_some_and(|at| at.is_none_or(|at| Instant::now() < at));
    if live {
        return Json(json!({ "ok": true })).into_response();
    }
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Bearer error=\"invalid_token\"")],
    )
        .into_response()
}
