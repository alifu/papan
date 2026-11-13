use std::sync::Mutex;
use std::time::Duration;

use serde_json::json;

use super::super::fixtures::{
    self, bearer, collection, Endpoint, NoBrowser, CLIENT_SECRET, COLLECTION,
};
use super::super::flow::token_for_send;
use super::super::{config_key, tokens, OAuthConfig};
use super::*;
use crate::store::types::Auth;
use crate::vars::vault::SecretStore;

const AUTH: &str = "http://login.example.test/authorize?tenant=a#frag";
const FLOW: &str = "11111111-2222-4333-8444-555555555555";
const OTHER_FLOW: &str = "99999999-2222-4333-8444-555555555555";

/// Plays the provider: answers the authorize URL by calling the redirect URI.
#[derive(Default)]
struct Provider {
    opened: Mutex<Vec<Url>>,
    /// `error` instead of `code`.
    deny: bool,
    /// Call the redirect URI with a wrong `state` first.
    forge_first: bool,
    /// Never call back.
    silent: bool,
}

fn query(url: &Url, name: &str) -> String {
    url.query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
        .unwrap_or_default()
}

impl BrowserOpener for Provider {
    fn open(&self, url: &Url) -> Result<(), AppError> {
        self.opened.lock().unwrap().push(url.clone());
        if self.silent {
            return Ok(());
        }
        let redirect = query(url, "redirect_uri");
        let state = query(url, "state");
        let answer = if self.deny {
            "error=access_denied&error_description=The+user+said+no"
        } else {
            "code=unit-auth-code"
        };
        let forged = self
            .forge_first
            .then(|| format!("{redirect}?code=forged-code&state=forged-state"));
        let real = format!("{redirect}?{answer}&state={state}");
        tokio::spawn(async move {
            if let Some(forged) = forged {
                let reply = reqwest::get(forged).await.unwrap();
                assert_eq!(reply.status().as_u16(), 400);
            }
            reqwest::get(real).await.unwrap();
        });
        Ok(())
    }
}

struct Setup {
    oauth: OAuth,
    vault: Arc<dyn SecretStore>,
    endpoint: Endpoint,
    location: RequestLocation,
}

async fn setup() -> Setup {
    let (oauth, vault) = fixtures::oauth();
    Setup {
        oauth,
        vault,
        endpoint: fixtures::endpoint().await,
        location: collection(COLLECTION),
    }
}

impl Setup {
    fn resolved(&self, grant: OAuthGrant) -> Resolved {
        fixtures::resolved(grant, AUTH, &self.endpoint.url())
    }

    async fn authorize(
        &self,
        resolved: &Resolved,
        opener: &dyn BrowserOpener,
    ) -> Result<OAuthStatus, AppError> {
        let key = key_of(resolved);
        self.oauth
            .authorize(FLOW, &self.location, resolved, &key, opener)
            .await
    }

    fn stored(&self, resolved: &Resolved) -> Option<tokens::TokenRecord> {
        let owner = tokens::Owner::of(&self.location).unwrap();
        tokens::load(&*self.vault, &owner, &key_of(resolved)).unwrap()
    }
}

fn key_of(resolved: &Resolved) -> String {
    config_key(&OAuthConfig::from_resolved(resolved).unwrap().unwrap())
}

#[tokio::test]
async fn client_credentials_store_a_token_without_a_browser() {
    let setup = setup().await;
    let resolved = setup.resolved(OAuthGrant::ClientCredentials);
    setup
        .endpoint
        .reply(200, &bearer("cc-access", None, Some(3600)));
    let browser = NoBrowser::default();

    let status = setup.authorize(&resolved, &browser).await.unwrap();

    assert!(status.has_token && status.can_refresh);
    assert!(!serde_json::to_string(&status)
        .unwrap()
        .contains("cc-access"));
    assert!(browser.0.lock().unwrap().is_empty());
    assert_eq!(setup.stored(&resolved).unwrap().access_token, "cc-access");
    assert_eq!(setup.oauth.pending(), 0);
    let net = setup.oauth.network_config().await.unwrap();
    let sent = token_for_send(&setup.oauth, &net, &setup.location, &resolved).await;
    assert!(sent.is_ok());
    assert_eq!(setup.endpoint.hits().len(), 1);
}

#[tokio::test]
async fn authorization_code_exchanges_the_code_with_the_verifier_of_the_challenge() {
    let setup = setup().await;
    let resolved = setup.resolved(OAuthGrant::AuthorizationCode);
    setup.endpoint.reply(
        200,
        &bearer("code-access", Some("code-refresh"), Some(3600)),
    );
    let provider = Provider {
        forge_first: true,
        ..Provider::default()
    };

    let status = setup.authorize(&resolved, &provider).await.unwrap();

    assert!(status.has_token && status.can_refresh);
    let opened = provider.opened.lock().unwrap().clone();
    assert_eq!(opened.len(), 1);
    let url = &opened[0];
    assert_eq!(url.host_str(), Some("login.example.test"));
    assert_eq!(url.fragment(), None);
    assert_eq!(query(url, "tenant"), "a");
    assert_eq!(query(url, "response_type"), "code");
    assert_eq!(query(url, "client_id"), fixtures::CLIENT_ID);
    assert_eq!(query(url, "scope"), "read write");
    assert_eq!(query(url, "code_challenge_method"), "S256");
    assert_eq!(query(url, "state").len(), 43);
    assert!(!url.as_str().contains(CLIENT_SECRET));
    let redirect = query(url, "redirect_uri");
    assert!(redirect.starts_with("http://127.0.0.1:") && redirect.ends_with("/callback"));
    let hits = setup.endpoint.hits();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].param("grant_type"), Some("authorization_code"));
    assert_eq!(hits[0].param("code"), Some("unit-auth-code"));
    assert_eq!(hits[0].param("redirect_uri"), Some(redirect.as_str()));
    let verifier = hits[0].param("code_verifier").unwrap();
    assert_eq!(verifier.len(), 43);
    assert_eq!(pkce::challenge(verifier), query(url, "code_challenge"));
    assert_ne!(verifier, query(url, "state"));
    let stored = setup.stored(&resolved).unwrap();
    assert_eq!(stored.refresh_token.as_deref(), Some("code-refresh"));
    assert_eq!(stored.token_url, setup.endpoint.url());
    // The listener is closed once the flow has ended.
    let port = Url::parse(&redirect).unwrap().port().unwrap();
    assert!(tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_err());
}

#[tokio::test]
async fn a_denied_sign_in_fails_with_the_provider_text_and_no_token_request() {
    let setup = setup().await;
    let resolved = setup.resolved(OAuthGrant::AuthorizationCode);
    let provider = Provider {
        deny: true,
        ..Provider::default()
    };

    let error = setup.authorize(&resolved, &provider).await.unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(error.message.contains("access_denied The user said no"));
    assert!(setup.endpoint.hits().is_empty());
    assert_eq!(setup.stored(&resolved), None);
    assert_eq!(setup.oauth.pending(), 0);
}

#[tokio::test]
async fn a_changed_configuration_is_refused_before_the_opener_or_the_network() {
    let setup = setup().await;
    let shown = setup.resolved(OAuthGrant::AuthorizationCode);
    let now = fixtures::resolved(
        OAuthGrant::AuthorizationCode,
        "http://evil.example.test/authorize",
        &setup.endpoint.url(),
    );
    let browser = NoBrowser::default();

    let error = setup
        .oauth
        .authorize(FLOW, &setup.location, &now, &key_of(&shown), &browser)
        .await
        .unwrap_err();

    assert!(error.message.contains("changed"));
    assert!(browser.0.lock().unwrap().is_empty());
    assert!(setup.endpoint.hits().is_empty());
}

#[tokio::test]
async fn urls_with_another_scheme_are_refused_before_the_opener_is_called() {
    let setup = setup().await;
    let browser = NoBrowser::default();
    let token = setup.endpoint.url();
    let cases = [
        fixtures::resolved(OAuthGrant::AuthorizationCode, "file:///etc/passwd", &token),
        fixtures::resolved(OAuthGrant::AuthorizationCode, "myapp://authorize", &token),
        fixtures::resolved(OAuthGrant::ClientCredentials, "", "file:///etc/passwd"),
    ];

    for resolved in cases {
        let error = setup
            .oauth
            .authorize(FLOW, &setup.location, &resolved, &"0".repeat(64), &browser)
            .await
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::InvalidInput);
    }
    assert!(browser.0.lock().unwrap().is_empty());
    assert!(setup.endpoint.hits().is_empty());
}

#[tokio::test]
async fn a_flow_id_that_is_not_a_uuid_is_refused() {
    let setup = setup().await;
    let resolved = setup.resolved(OAuthGrant::ClientCredentials);
    let key = key_of(&resolved);

    for flow_id in ["", "flow-1", "../x", "AAAAAAAA-2222-4333-8444-555555555555"] {
        let result = setup
            .oauth
            .authorize(
                flow_id,
                &setup.location,
                &resolved,
                &key,
                &NoBrowser::default(),
            )
            .await;

        assert_eq!(
            result.unwrap_err().code,
            ErrorCode::InvalidInput,
            "{flow_id}"
        );
        assert!(setup.oauth.cancel(flow_id).is_err());
    }
    assert!(setup.endpoint.hits().is_empty());
}

#[tokio::test]
async fn cancel_ends_a_pending_flow_and_closes_the_listener() {
    let setup = setup().await;
    let resolved = setup.resolved(OAuthGrant::AuthorizationCode);
    let provider = Provider {
        silent: true,
        ..Provider::default()
    };

    let flow = setup.authorize(&resolved, &provider);
    let cancel = async {
        while provider.opened.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let duplicate = setup.authorize(&resolved, &provider).await;
        setup.oauth.cancel(OTHER_FLOW).unwrap();
        assert_eq!(setup.oauth.pending(), 1);
        setup.oauth.cancel(FLOW).unwrap();
        duplicate
    };
    let (result, duplicate) = tokio::join!(flow, cancel);

    assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
    assert!(duplicate.unwrap_err().message.contains("already running"));
    assert_eq!(setup.oauth.pending(), 0);
    let opened = provider.opened.lock().unwrap().clone();
    let port = Url::parse(&query(&opened[0], "redirect_uri"))
        .unwrap()
        .port()
        .unwrap();
    assert!(tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_err());
    assert!(setup.endpoint.hits().is_empty());
}

#[tokio::test]
async fn a_flow_ends_with_a_timeout_after_its_lifetime() {
    let mut setup = setup().await;
    setup.oauth = setup.oauth.with_lifetime(Duration::from_millis(150));
    let resolved = setup.resolved(OAuthGrant::AuthorizationCode);
    let provider = Provider {
        silent: true,
        ..Provider::default()
    };

    let error = setup.authorize(&resolved, &provider).await.unwrap_err();

    assert_eq!(error.code, ErrorCode::Timeout);
    assert_eq!(setup.oauth.pending(), 0);
    let opened = provider.opened.lock().unwrap().clone();
    let port = Url::parse(&query(&opened[0], "redirect_uri"))
        .unwrap()
        .port()
        .unwrap();
    assert!(tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_err());
}

#[tokio::test]
async fn a_busy_fixed_redirect_port_is_refused_before_the_opener() {
    let setup = setup().await;
    let busy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = busy.local_addr().unwrap().port();
    let mut auth = fixtures::oauth2(OAuthGrant::AuthorizationCode, AUTH, &setup.endpoint.url());
    if let Auth::Oauth2 { redirect_port, .. } = &mut auth {
        *redirect_port = port;
    }
    let resolved = fixtures::resolved_with(auth);
    let browser = NoBrowser::default();

    let error = setup.authorize(&resolved, &browser).await.unwrap_err();

    assert!(error.message.contains(&port.to_string()));
    assert!(browser.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_failed_exchange_masks_the_code_the_verifier_and_the_client_secret() {
    let setup = setup().await;
    let resolved = setup.resolved(OAuthGrant::AuthorizationCode);
    setup.endpoint.reply(
        400,
        &json!({
            "error": "invalid_grant",
            "error_description": format!("code unit-auth-code secret {CLIENT_SECRET}")
        }),
    );

    let error = setup
        .authorize(&resolved, &Provider::default())
        .await
        .unwrap_err();

    let text = format!("{error:?}");
    assert!(error.message.contains("invalid_grant"));
    assert!(!text.contains("unit-auth-code") && !text.contains(CLIENT_SECRET));
    let verifier = setup.endpoint.hits()[0]
        .param("code_verifier")
        .unwrap()
        .to_string();
    assert!(!text.contains(&verifier));
    assert_eq!(setup.stored(&resolved), None);
}

#[tokio::test]
async fn an_opener_failure_ends_the_flow() {
    let setup = setup().await;
    let resolved = setup.resolved(OAuthGrant::AuthorizationCode);
    let browser = NoBrowser::default();

    let error = setup.authorize(&resolved, &browser).await.unwrap_err();

    assert_eq!(error.code, ErrorCode::Internal);
    assert_eq!(browser.0.lock().unwrap().len(), 1);
    assert_eq!(setup.oauth.pending(), 0);
}
