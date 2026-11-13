use std::time::Duration;

use serde_json::json;

use super::super::fixtures::{
    self, bearer, collection, Endpoint, CLIENT_SECRET, COLLECTION, OTHER_COLLECTION,
};
use super::*;
use crate::error::ErrorCode;
use crate::store::types::RequestContent;
use crate::vars::vault::SecretStore;

const AUTH: &str = "https://login.example.test/authorize";

struct Setup {
    oauth: OAuth,
    vault: Arc<dyn SecretStore>,
    endpoint: Endpoint,
    net: Arc<NetConfig>,
    location: RequestLocation,
}

async fn setup() -> Setup {
    let (oauth, vault) = fixtures::oauth();
    let net = oauth.network_config().await.unwrap();
    Setup {
        oauth,
        vault,
        endpoint: fixtures::endpoint().await,
        net,
        location: collection(COLLECTION),
    }
}

impl Setup {
    fn resolved(&self, grant: OAuthGrant) -> Resolved {
        fixtures::resolved(grant, AUTH, &self.endpoint.url())
    }

    /// Stores a record for `resolved` that expires `expires_in_ms` from now.
    fn seed(&self, resolved: &Resolved, refresh: Option<&str>, expires_in_ms: Option<i64>) {
        let target = Target::of(&self.location, resolved).unwrap();
        let record = TokenRecord {
            access_token: "stored-access".to_string(),
            refresh_token: refresh.map(str::to_string),
            expires_at_ms: expires_in_ms.map(|ms| now().saturating_add_signed(ms)),
            scope: "read write".to_string(),
            token_url: target.config.token_url.to_string(),
            saved_at_ms: now(),
            verify_tls: true,
        };
        tokens::save(&*self.vault, &target.owner, &target.key, &record).unwrap();
    }

    fn stored(&self, resolved: &Resolved) -> Option<TokenRecord> {
        let target = Target::of(&self.location, resolved).unwrap();
        tokens::load(&*self.vault, &target.owner, &target.key).unwrap()
    }

    async fn send(&self, resolved: &Resolved) -> Result<Resolved, AppError> {
        token_for_send(&self.oauth, &self.net, &self.location, resolved).await
    }
}

fn bearer_of(resolved: &Resolved) -> &str {
    match &resolved.content.auth {
        Auth::Bearer { token } => token,
        other => panic!("expected bearer auth, got {other:?}"),
    }
}

#[tokio::test]
async fn a_draft_without_oauth_is_returned_unchanged() {
    let setup = setup().await;
    let plain = Resolved::unchanged(RequestContent::new("GET", "https://example.test"));

    let sent = setup.send(&plain).await.unwrap();

    assert_eq!(sent, plain);
}

#[tokio::test]
async fn send_without_a_record_is_refused_and_makes_no_token_request() {
    let setup = setup().await;
    setup
        .endpoint
        .reply(200, &bearer("never-issued", None, None));

    for grant in [OAuthGrant::ClientCredentials, OAuthGrant::AuthorizationCode] {
        let error = setup.send(&setup.resolved(grant)).await.unwrap_err();

        assert_eq!(error.code, ErrorCode::InvalidInput);
        assert_eq!(error.message, NO_TOKEN_MESSAGE);
    }
    assert!(setup.endpoint.hits().is_empty());
}

#[tokio::test]
async fn a_valid_token_is_attached_as_bearer_and_masked_without_a_request() {
    let setup = setup().await;
    let resolved = setup.resolved(OAuthGrant::AuthorizationCode);
    setup.seed(&resolved, Some("stored-refresh"), Some(3_600_000));

    let sent = setup.send(&resolved).await.unwrap();

    assert_eq!(bearer_of(&sent), "stored-access");
    assert!(sent.secret_values.contains(&"stored-access".to_string()));
    assert!(sent.secret_values.contains(&CLIENT_SECRET.to_string()));
    assert_eq!(sent.content.url, resolved.content.url);
    assert!(setup.endpoint.hits().is_empty());
}

#[tokio::test]
async fn a_token_without_an_expiry_is_always_valid() {
    let setup = setup().await;
    let resolved = setup.resolved(OAuthGrant::ClientCredentials);
    setup.seed(&resolved, None, None);

    let sent = setup.send(&resolved).await.unwrap();

    assert_eq!(bearer_of(&sent), "stored-access");
    assert!(setup.endpoint.hits().is_empty());
}

#[tokio::test]
async fn an_expired_token_is_refreshed_once_under_ten_concurrent_sends() {
    let setup = setup().await;
    let resolved = setup.resolved(OAuthGrant::AuthorizationCode);
    setup.seed(&resolved, Some("stored-refresh"), Some(-1000));
    setup.endpoint.delay(Duration::from_millis(100));
    setup.endpoint.reply(
        200,
        &bearer("renewed-access", Some("rotated-refresh"), Some(3600)),
    );

    let sends = (0..10).map(|_| setup.send(&resolved));
    let results = futures_util::future::join_all(sends).await;

    for result in results {
        assert_eq!(bearer_of(&result.unwrap()), "renewed-access");
    }
    let hits = setup.endpoint.hits();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].param("grant_type"), Some("refresh_token"));
    assert_eq!(hits[0].param("refresh_token"), Some("stored-refresh"));
    let stored = setup.stored(&resolved).unwrap();
    assert_eq!(stored.access_token, "renewed-access");
    assert_eq!(stored.refresh_token.as_deref(), Some("rotated-refresh"));
    assert!(stored.expires_at_ms.unwrap() > now() + 3_500_000);
}

#[tokio::test]
async fn a_token_inside_the_skew_window_is_refreshed() {
    let setup = setup().await;
    let resolved = setup.resolved(OAuthGrant::AuthorizationCode);
    setup.seed(&resolved, Some("stored-refresh"), Some(20_000));
    setup
        .endpoint
        .reply(200, &bearer("renewed-access", None, Some(3600)));

    let sent = setup.send(&resolved).await.unwrap();

    assert_eq!(bearer_of(&sent), "renewed-access");
    let stored = setup.stored(&resolved).unwrap();
    assert_eq!(stored.refresh_token.as_deref(), Some("stored-refresh"));
    assert_eq!(stored.scope, "read write");
}

#[tokio::test]
async fn expired_client_credentials_are_requested_again_because_a_record_exists() {
    let setup = setup().await;
    let resolved = setup.resolved(OAuthGrant::ClientCredentials);
    setup.seed(&resolved, None, Some(-1000));
    setup
        .endpoint
        .reply(200, &bearer("second-access", None, Some(3600)));

    let sent = setup.send(&resolved).await.unwrap();

    assert_eq!(bearer_of(&sent), "second-access");
    let hits = setup.endpoint.hits();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].param("grant_type"), Some("client_credentials"));
}

#[tokio::test]
async fn an_expired_code_token_without_a_refresh_token_asks_for_a_new_one() {
    let setup = setup().await;
    let resolved = setup.resolved(OAuthGrant::AuthorizationCode);
    setup.seed(&resolved, None, Some(-1000));

    let error = setup.send(&resolved).await.unwrap_err();

    assert_eq!(error.message, NO_TOKEN_MESSAGE);
    assert!(setup.endpoint.hits().is_empty());
}

#[tokio::test]
async fn a_refresh_token_is_never_sent_to_another_token_url() {
    let setup = setup().await;
    let other = fixtures::endpoint().await;
    let resolved = setup.resolved(OAuthGrant::AuthorizationCode);
    setup.seed(&resolved, Some("stored-refresh"), Some(-1000));
    let moved = fixtures::resolved(OAuthGrant::AuthorizationCode, AUTH, &other.url());

    let after_change = setup.send(&moved).await.unwrap_err();

    assert_eq!(after_change.message, NO_TOKEN_MESSAGE);
    assert!(other.hits().is_empty() && setup.endpoint.hits().is_empty());

    // Even a record planted under the new key with a foreign URL is not used.
    let target = Target::of(&setup.location, &moved).unwrap();
    let mut planted = setup.stored(&resolved).unwrap();
    planted.token_url = setup.endpoint.url();
    planted.expires_at_ms = Some(1);
    tokens::save(&*setup.vault, &target.owner, &target.key, &planted).unwrap();

    let planted_send = setup.send(&moved).await.unwrap_err();

    assert_eq!(planted_send.message, NO_TOKEN_MESSAGE);
    assert!(other.hits().is_empty() && setup.endpoint.hits().is_empty());
}

#[tokio::test]
async fn another_collection_and_scratch_cannot_use_the_token() {
    let setup = setup().await;
    let resolved = setup.resolved(OAuthGrant::AuthorizationCode);
    setup.seed(&resolved, Some("stored-refresh"), Some(3_600_000));
    let scratch = RequestLocation::Scratch {
        id: "s1".to_string(),
    };

    for location in [collection(OTHER_COLLECTION), scratch] {
        let error = token_for_send(&setup.oauth, &setup.net, &location, &resolved)
            .await
            .unwrap_err();

        assert_eq!(error.message, NO_TOKEN_MESSAGE);
    }
    let invalid = token_for_send(&setup.oauth, &setup.net, &collection("../x"), &resolved).await;
    assert_eq!(invalid.unwrap_err().code, ErrorCode::InvalidInput);
}

#[tokio::test]
async fn a_failed_refresh_keeps_the_record_and_masks_every_secret() {
    let setup = setup().await;
    let resolved = setup.resolved(OAuthGrant::AuthorizationCode);
    setup.seed(&resolved, Some("stored-refresh"), Some(-1000));
    setup.endpoint.reply(
        503,
        &json!({
            "error": "temporarily_unavailable",
            "error_description":
                format!("stored-refresh stored-access {CLIENT_SECRET} is not good")
        }),
    );

    let error = setup.send(&resolved).await.unwrap_err();

    let text = format!("{error:?}");
    assert!(error.message.contains("temporarily_unavailable"));
    assert!(!text.contains("stored-refresh") && !text.contains("stored-access"));
    assert!(!text.contains(CLIENT_SECRET));
    assert_eq!(
        setup.stored(&resolved).unwrap().access_token,
        "stored-access"
    );
}

#[tokio::test]
async fn a_refused_token_type_on_refresh_is_an_error() {
    let setup = setup().await;
    let resolved = setup.resolved(OAuthGrant::ClientCredentials);
    setup.seed(&resolved, None, Some(-1000));
    setup.endpoint.reply(
        200,
        &json!({ "access_token": "mac-access", "token_type": "mac" }),
    );

    let error = setup.send(&resolved).await.unwrap_err();

    assert!(error.message.contains("bearer"));
    assert!(!format!("{error:?}").contains("mac-access"));
    assert_eq!(
        setup.stored(&resolved).unwrap().access_token,
        "stored-access"
    );
}

#[tokio::test]
async fn an_unresolved_variable_refuses_the_send_before_anything_else() {
    let setup = setup().await;
    let unresolved = Resolved {
        unresolved: vec!["idp".to_string()],
        ..fixtures::resolved(OAuthGrant::ClientCredentials, "", "{{idp}}/token")
    };

    let error = setup.send(&unresolved).await.unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(error.message.contains("`idp`"));
}

#[tokio::test]
async fn status_reports_the_record_without_any_token() {
    let setup = setup().await;
    let resolved = setup.resolved(OAuthGrant::AuthorizationCode);

    let before = setup
        .oauth
        .status(&setup.location, &resolved)
        .await
        .unwrap();
    setup.seed(&resolved, Some("stored-refresh"), Some(3_600_000));
    let after = setup
        .oauth
        .status(&setup.location, &resolved)
        .await
        .unwrap();

    assert!(!before.has_token && !before.can_refresh);
    assert_eq!(before.expires_at_ms, None);
    assert_eq!(before.auth_host.as_deref(), Some("login.example.test"));
    assert_eq!(before.token_host, setup.endpoint.addr.to_string());
    assert!(before.plaintext);
    assert_eq!(
        before.redirect_uri.as_deref(),
        Some("http://127.0.0.1/callback")
    );
    assert_eq!(before.config_key.len(), 64);
    assert!(after.has_token && after.can_refresh);
    assert!(after.expires_at_ms.is_some());
    assert_eq!(after.config_key, before.config_key);
    let json = serde_json::to_string(&after).unwrap();
    assert!(!json.contains("stored-access") && !json.contains("stored-refresh"));
    assert!(!json.contains(CLIENT_SECRET));
    assert!(setup.endpoint.hits().is_empty());
}

#[tokio::test]
async fn status_of_client_credentials_has_no_browser_fields() {
    let setup = setup().await;
    let resolved = setup.resolved(OAuthGrant::ClientCredentials);
    setup.seed(&resolved, None, Some(3_600_000));

    let status = setup
        .oauth
        .status(&setup.location, &resolved)
        .await
        .unwrap();

    assert_eq!(status.auth_host, None);
    assert_eq!(status.redirect_uri, None);
    assert!(status.has_token && status.can_refresh);
}

#[tokio::test]
async fn clear_forgets_the_token_so_the_next_send_is_refused() {
    let setup = setup().await;
    let resolved = setup.resolved(OAuthGrant::ClientCredentials);
    setup.seed(&resolved, None, Some(3_600_000));

    let status = setup.oauth.clear(&setup.location, &resolved).await.unwrap();

    assert!(!status.has_token);
    assert_eq!(setup.stored(&resolved), None);
    assert_eq!(
        setup.send(&resolved).await.unwrap_err().message,
        NO_TOKEN_MESSAGE
    );
    assert!(setup.endpoint.hits().is_empty());
}

#[tokio::test]
async fn status_and_clear_refuse_a_draft_without_oauth() {
    let setup = setup().await;
    let plain = Resolved::unchanged(RequestContent::new("GET", "https://example.test"));

    let status = setup.oauth.status(&setup.location, &plain).await;
    let clear = setup.oauth.clear(&setup.location, &plain).await;

    assert_eq!(status.unwrap_err().code, ErrorCode::InvalidInput);
    assert_eq!(clear.unwrap_err().code, ErrorCode::InvalidInput);
}
