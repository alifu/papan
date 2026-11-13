//! OAuth 2 through the real send path (plan 03b §5.3 rules 2, 3, 6, 7): variables are
//! resolved, the stored token is attached as Bearer, history is recorded.

mod support;

#[allow(dead_code)]
#[path = "oauth/app.rs"]
mod app;

use std::sync::Arc;
use std::time::Duration;

use app::{capture_logs, draft, key_of, logs, FakeBrowser};
use papan_lib::engine::http::oauth::flow::NO_TOKEN_MESSAGE;
use papan_lib::engine::http::resolve::Resolved;
use papan_lib::engine::http::types::ResponseMeta;
use papan_lib::engine::http::variables::send_with_variables;
use papan_lib::engine::http::HttpEngine;
use papan_lib::error::{AppError, ErrorCode};
use papan_lib::store::types::{KeyValue, OAuthGrant, RequestContent, RequestLocation};
use papan_lib::store::{Store, StoreState};
use papan_lib::vars::dynamic::SystemDynamics;
use papan_lib::vars::types::{SecretEdit, VarScope, VariableInput};
use papan_lib::vars::vault::MemoryVault;
use papan_lib::vars::VarsService;
use serde_json::Value;
use support::idp::{self, IdpConfig, TestIdp, CLIENT_SECRET};
use support::server;

const CREDENTIALS: OAuthGrant = OAuthGrant::ClientCredentials;
const CODE: OAuthGrant = OAuthGrant::AuthorizationCode;
const USER_AUTHORIZATION: &str = "Bearer typed-by-the-user";
const LONG_LIFETIME: Duration = Duration::from_secs(3600);

/// A provider whose tokens are already inside the expiry skew when issued.
fn short_lived() -> IdpConfig {
    IdpConfig {
        token_lifetime: Some(Duration::from_secs(1)),
        ..IdpConfig::default()
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    engine: HttpEngine,
    store: StoreState,
    vars: VarsService,
    location: RequestLocation,
}

fn fixture() -> Fixture {
    capture_logs();
    let dir = tempfile::tempdir().unwrap();
    let engine = HttpEngine::new(&dir.path().join("cache"), dir.path(), "9.9.9").unwrap();
    let mut store = Store::new(&dir.path().join("data")).unwrap();
    let tree = store.create_collection(dir.path(), "C").unwrap();
    Fixture {
        location: RequestLocation::Collection {
            collection_id: tree.collection.id,
            path: "r.http.yaml".into(),
        },
        _dir: dir,
        engine,
        store: StoreState::new(store),
        vars: VarsService::new(Arc::new(MemoryVault::default())),
    }
}

impl Fixture {
    /// "Get new token" for `resolved`; the fake browser completes an authorization code.
    async fn get_token(&self, resolved: &Resolved) {
        let flow_id = uuid::Uuid::new_v4().to_string();
        self.engine
            .oauth()
            .authorize(
                &flow_id,
                &self.location,
                resolved,
                &key_of(resolved),
                &FakeBrowser::default(),
            )
            .await
            .unwrap();
    }

    async fn send(&self, content: &RequestContent) -> (String, Result<ResponseMeta, AppError>) {
        let id = uuid::Uuid::new_v4().to_string();
        let result = send_with_variables(
            &self.engine,
            &self.store,
            &self.vars,
            Arc::new(SystemDynamics),
            &id,
            &self.location,
            content,
        )
        .await;
        (id, result)
    }

    /// The `authorization` header the echo server received.
    async fn echoed_authorization(&self, content: &RequestContent) -> Option<String> {
        let (id, result) = self.send(content).await;
        result.unwrap();
        let echo: Value = serde_json::from_str(&self.engine.read_body(&id).unwrap().data).unwrap();
        echo["headers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|pair| pair[0] == "authorization")
            .map(|pair| pair[1].as_str().unwrap().to_string())
    }

    fn define_in_collection(&self, name: &str, value: &str) {
        let RequestLocation::Collection { collection_id, .. } = &self.location else {
            unreachable!("the fixture request is in a collection")
        };
        let row = VariableInput {
            name: name.into(),
            previous_name: None,
            value: value.into(),
            current: None,
            enabled: true,
            secret: false,
            secret_edit: SecretEdit::Keep,
        };
        let scope = VarScope::Collection {
            collection_id: collection_id.clone(),
        };
        self.vars
            .write_variables(&mut self.store.lock_store(), &scope, &[row])
            .unwrap();
    }

    fn history_text(&self) -> String {
        serde_json::to_string(&self.store.lock_store().list_history()).unwrap()
    }
}

/// The OAuth 2 draft of `idp`, sent to `url` instead of its `/resource`.
fn draft_to(idp: &TestIdp, grant: OAuthGrant, url: String) -> RequestContent {
    RequestContent {
        url,
        ..draft(idp, grant).content
    }
}

fn token_hits_of(idp: &TestIdp, grant_type: &str) -> usize {
    idp.token_hits()
        .iter()
        .filter(|hit| hit.param("grant_type") == Some(grant_type))
        .count()
}

#[tokio::test]
async fn send_without_a_token_is_refused_and_the_provider_is_not_called() {
    let idp = idp::spawn(IdpConfig::default()).await;
    let f = fixture();

    let (_, result) = f.send(&draft(&idp, CREDENTIALS).content).await;

    let error = result.unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert_eq!(error.message, NO_TOKEN_MESSAGE);
    assert!(idp.hits().is_empty(), "{:?}", idp.hits().len());
}

#[tokio::test]
async fn a_valid_token_reaches_the_resource() {
    let idp = idp::spawn(IdpConfig::default()).await;
    let f = fixture();
    let resolved = draft(&idp, CREDENTIALS);
    f.get_token(&resolved).await;

    let (_, result) = f.send(&resolved.content).await;

    assert_eq!(result.unwrap().status, 200);
    assert_eq!(idp.token_hits().len(), 1);
}

#[tokio::test]
async fn an_expired_token_is_refreshed_once_and_the_send_succeeds() {
    let idp = idp::spawn(short_lived()).await;
    let f = fixture();
    let resolved = draft(&idp, CODE);
    f.get_token(&resolved).await;
    idp.configure(|config| config.token_lifetime = Some(LONG_LIFETIME));

    let (_, first) = f.send(&resolved.content).await;
    let (_, second) = f.send(&resolved.content).await;

    assert_eq!(first.unwrap().status, 200);
    assert_eq!(second.unwrap().status, 200);
    assert_eq!(token_hits_of(&idp, "refresh_token"), 1);
    assert_eq!(idp.token_hits().len(), 2);
}

#[tokio::test]
async fn a_user_authorization_header_wins_over_the_token() {
    let idp = idp::spawn(IdpConfig::default()).await;
    let echo = server::spawn().await;
    let f = fixture();
    f.get_token(&draft(&idp, CREDENTIALS)).await;
    let content = RequestContent {
        headers: vec![KeyValue {
            name: "Authorization".into(),
            value: USER_AUTHORIZATION.into(),
            enabled: true,
        }],
        ..draft_to(&idp, CREDENTIALS, echo.url("/echo"))
    };

    let authorization = f.echoed_authorization(&content).await;

    assert_eq!(authorization.as_deref(), Some(USER_AUTHORIZATION));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cross_origin_redirect_drops_the_bearer() {
    let idp = idp::spawn(IdpConfig::default()).await;
    let first = server::spawn().await;
    let second = server::spawn().await;
    let f = fixture();
    f.get_token(&draft(&idp, CREDENTIALS)).await;
    let direct = draft_to(&idp, CREDENTIALS, second.url("/echo"));
    let redirected = draft_to(
        &idp,
        CREDENTIALS,
        first.url(&format!("/redirect-to?url={}", second.url("/echo"))),
    );

    let sent_directly = f.echoed_authorization(&direct).await;
    let after_redirect = f.echoed_authorization(&redirected).await;

    let token = idp.issued()[0].clone();
    assert_eq!(sent_directly, Some(format!("Bearer {token}")));
    assert_eq!(after_redirect, None);
}

#[tokio::test]
async fn tokens_and_the_client_secret_stay_out_of_history_logs_errors_and_the_url() {
    let idp = idp::spawn(short_lived()).await;
    let f = fixture();
    f.get_token(&draft(&idp, CODE)).await;
    idp.configure(|config| config.token_lifetime = Some(LONG_LIFETIME));
    // A plain variable: only the OAuth hook knows its value must be masked.
    f.define_in_collection("s", CLIENT_SECRET);
    let shown = draft_to(&idp, CODE, idp.url("/resource?s={{s}}"));
    let refused = draft_to(&idp, CODE, "http://127.0.0.1:1/x?s={{s}}".to_string());

    let (_, ok) = f.send(&shown).await;
    let (_, failed) = f.send(&refused).await;

    let meta = ok.unwrap();
    assert_eq!(meta.status, 200);
    let error = failed.unwrap_err();
    let error_text = format!("{} {:?}", error.message, error.detail);
    let history = f.history_text();
    let logs = logs();
    let secrets: Vec<String> = idp
        .issued()
        .into_iter()
        .chain([CLIENT_SECRET.to_string()])
        .collect();
    assert!(secrets.len() >= 4, "{secrets:?}");
    assert!(history.contains("s={{s}}"), "{history}");
    assert!(logs.contains("sending request"), "nothing captured");
    for secret in &secrets {
        assert!(!meta.url.contains(secret.as_str()), "{}", meta.url);
        assert!(!error_text.contains(secret.as_str()), "{error_text}");
        assert!(!history.contains(secret.as_str()), "{history}");
        assert!(!logs.contains(secret.as_str()), "a secret was logged");
    }
}
