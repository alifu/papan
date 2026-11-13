//! OAuth 2 end to end against the test provider (plan 03b §5, §10): both grants, the
//! loopback listener, the send-time rule, single flight, token binding and leaks.

mod support;

#[path = "oauth/app.rs"]
mod app;
#[path = "oauth/listener.rs"]
mod listener;
#[path = "oauth/races.rs"]
mod races;
#[path = "oauth/renewal.rs"]
mod renewal;

use std::time::Duration;

use app::{collection, draft, draft_with, query, App, FakeBrowser, COLLECTION, OTHER_COLLECTION};
use papan_lib::engine::http::oauth::flow::NO_TOKEN_MESSAGE;
use papan_lib::engine::http::resolve::Resolved;
use papan_lib::error::ErrorCode;
use papan_lib::store::types::{Auth, ClientAuth, OAuthGrant, RequestLocation};
use support::idp::{self, IdpConfig, CLIENT_SECRET};

const CODE: OAuthGrant = OAuthGrant::AuthorizationCode;
const CREDENTIALS: OAuthGrant = OAuthGrant::ClientCredentials;

fn short_lived() -> IdpConfig {
    IdpConfig {
        token_lifetime: Some(Duration::from_secs(1)),
        ..IdpConfig::default()
    }
}

fn set_scope(auth: &mut Auth, value: &str) {
    if let Auth::Oauth2 { scope, .. } = auth {
        *scope = value.to_string();
    }
}

#[tokio::test]
async fn client_credentials_work_end_to_end_with_basic_and_body_authentication() {
    for (provider, ours) in [
        (idp::ClientAuth::Basic, ClientAuth::Basic),
        (idp::ClientAuth::Body, ClientAuth::Body),
    ] {
        let idp = idp::spawn(IdpConfig {
            client_auth: provider,
            ..IdpConfig::default()
        })
        .await;
        let app = App::new();
        let resolved = draft_with(&idp, CREDENTIALS, |auth| {
            if let Auth::Oauth2 { client_auth, .. } = auth {
                *client_auth = ours;
            }
        });
        let browser = FakeBrowser::default();

        let status = app.authorize(&resolved, &browser).await.unwrap();
        let meta = app.send(&resolved).await.unwrap();

        assert!(status.has_token && status.can_refresh);
        assert_eq!(status.scope, "read write");
        assert_eq!(meta.status, 200);
        assert!(browser.opened.lock().unwrap().is_empty());
        let hits = idp.token_hits();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param("grant_type"), Some("client_credentials"));
        assert_eq!(hits[0].cookie, None);
    }
}

#[tokio::test]
async fn authorization_code_works_end_to_end_with_an_s256_challenge() {
    let idp = idp::spawn(IdpConfig::default()).await;
    let app = App::new();
    let resolved = draft(&idp, CODE);
    let browser = FakeBrowser::default();

    let status = app.authorize(&resolved, &browser).await.unwrap();
    let meta = app.send(&resolved).await.unwrap();

    assert!(status.has_token && status.can_refresh);
    assert!(status.expires_at_ms.is_some());
    assert_eq!(meta.status, 200);
    let hits = idp.hits();
    let authorize = hits.iter().find(|hit| hit.path == "/authorize").unwrap();
    let token = hits.iter().find(|hit| hit.path == "/token").unwrap();
    let verifier = token.param("code_verifier").unwrap();
    assert_eq!(authorize.param("code_challenge_method"), Some("S256"));
    assert_eq!(
        authorize.param("code_challenge"),
        Some(idp::s256(verifier).as_str())
    );
    assert_eq!(authorize.param("state").unwrap().len(), 43);
    assert!(authorize
        .param("redirect_uri")
        .unwrap()
        .starts_with("http://127.0.0.1:"));
    assert_eq!(token.cookie, None);
    assert_eq!(idp.token_hits().len(), 1);
    // The listener is closed after success.
    let opened = browser.opened.lock().unwrap().clone();
    let redirect = url::Url::parse(&query(&opened[0], "redirect_uri")).unwrap();
    let port = redirect.port().unwrap();
    assert!(tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_err());
}

#[tokio::test]
async fn a_provider_error_fails_the_flow_with_its_text() {
    let idp = idp::spawn(IdpConfig {
        authorize_error: Some("access_denied".to_string()),
        ..IdpConfig::default()
    })
    .await;
    let app = App::new();
    let resolved = draft(&idp, CODE);

    let error = app
        .authorize(&resolved, &FakeBrowser::default())
        .await
        .unwrap_err();

    assert!(error.message.contains("access_denied"));
    assert!(idp.token_hits().is_empty());
    assert_eq!(
        app.attach(&resolved).await.unwrap_err().message,
        NO_TOKEN_MESSAGE
    );
}

#[tokio::test]
async fn send_with_no_record_makes_zero_calls_to_the_token_endpoint() {
    let idp = idp::spawn(IdpConfig::default()).await;
    let app = App::new();

    for grant in [CODE, CREDENTIALS] {
        let error = app.send(&draft(&idp, grant)).await.unwrap_err();

        assert_eq!(error.code, ErrorCode::InvalidInput);
        assert_eq!(error.message, NO_TOKEN_MESSAGE);
    }
    assert!(idp.hits().is_empty());
}

#[tokio::test]
async fn dev_and_prod_configurations_hold_separate_tokens() {
    let dev = idp::spawn(IdpConfig::default()).await;
    let prod = idp::spawn(IdpConfig::default()).await;
    let app = App::new();
    let (dev_draft, prod_draft) = (draft(&dev, CREDENTIALS), draft(&prod, CREDENTIALS));

    app.authorize(&dev_draft, &FakeBrowser::default())
        .await
        .unwrap();
    let prod_before = app.attach(&prod_draft).await;
    app.authorize(&prod_draft, &FakeBrowser::default())
        .await
        .unwrap();

    assert_eq!(prod_before.unwrap_err().message, NO_TOKEN_MESSAGE);
    let dev_token = app.attach(&dev_draft).await.unwrap().content.auth;
    let prod_token = app.attach(&prod_draft).await.unwrap().content.auth;
    assert_ne!(dev_token, prod_token);
    assert_eq!(app.send(&dev_draft).await.unwrap().status, 200);
    assert_eq!(app.send(&prod_draft).await.unwrap().status, 200);
    assert_eq!(dev.token_hits().len(), 1);
    assert_eq!(prod.token_hits().len(), 1);
}

#[tokio::test]
async fn another_collection_cannot_read_the_token_and_clear_forgets_it() {
    let idp = idp::spawn(IdpConfig::default()).await;
    let app = App::new();
    let resolved = draft(&idp, CREDENTIALS);
    app.authorize(&resolved, &FakeBrowser::default())
        .await
        .unwrap();
    let scratch = RequestLocation::Scratch {
        id: "s".to_string(),
    };

    for location in [collection(OTHER_COLLECTION), scratch] {
        let error = app.attach_at(&location, &resolved).await.unwrap_err();
        let status = app.oauth().status(&location, &resolved).await.unwrap();

        assert_eq!(error.message, NO_TOKEN_MESSAGE);
        assert!(!status.has_token);
    }
    assert!(app.attach_at(&collection("../x"), &resolved).await.is_err());

    let cleared = app.oauth().clear(&app.location, &resolved).await.unwrap();

    assert!(!cleared.has_token);
    assert_eq!(
        app.attach(&resolved).await.unwrap_err().message,
        NO_TOKEN_MESSAGE
    );
    assert_eq!(idp.token_hits().len(), 1);
}

#[tokio::test]
async fn a_token_endpoint_redirect_is_not_followed() {
    let idp = idp::spawn(IdpConfig::default()).await;
    let app = App::new();
    let resolved = draft_with(&idp, CREDENTIALS, |auth| {
        if let Auth::Oauth2 { token_url, .. } = auth {
            *token_url = idp.url("/token-redirect");
        }
    });

    let error = app
        .authorize(&resolved, &FakeBrowser::default())
        .await
        .unwrap_err();

    assert!(error.message.contains("redirect"));
    let paths: Vec<String> = idp.hits().into_iter().map(|hit| hit.path).collect();
    assert_eq!(paths, ["/token-redirect"]);
    assert!(idp.issued().is_empty());
}

#[tokio::test]
async fn a_mac_token_type_is_refused_and_nothing_is_stored() {
    let idp = idp::spawn(IdpConfig {
        token_type: "mac".to_string(),
        ..IdpConfig::default()
    })
    .await;
    let app = App::new();
    let resolved = draft(&idp, CREDENTIALS);

    let error = app
        .authorize(&resolved, &FakeBrowser::default())
        .await
        .unwrap_err();

    assert!(error.message.contains("bearer"));
    assert!(!format!("{error:?}").contains(&idp.issued()[0]));
    assert_eq!(
        app.attach(&resolved).await.unwrap_err().message,
        NO_TOKEN_MESSAGE
    );
}

#[tokio::test]
async fn a_provider_refusal_surfaces_its_error_with_the_secret_masked() {
    let idp = idp::spawn(IdpConfig {
        token_error: Some(format!("bad_{CLIENT_SECRET}")),
        ..IdpConfig::default()
    })
    .await;
    let app = App::new();

    let error = app
        .authorize(&draft(&idp, CREDENTIALS), &FakeBrowser::default())
        .await
        .unwrap_err();

    assert!(error.message.contains("refused the request: bad_"));
    assert!(!format!("{error:?}").contains(CLIENT_SECRET));
}

#[tokio::test]
async fn file_and_custom_scheme_urls_are_refused_before_the_opener_is_called() {
    let idp = idp::spawn(IdpConfig::default()).await;
    let app = App::new();
    let browser = FakeBrowser::default();
    let set_auth = |url: &str| {
        let url = url.to_string();
        draft_with(&idp, CODE, move |auth| {
            if let Auth::Oauth2 { auth_url, .. } = auth {
                *auth_url = url;
            }
        })
    };
    let set_token = |url: &str| {
        let url = url.to_string();
        draft_with(&idp, CREDENTIALS, move |auth| {
            if let Auth::Oauth2 { token_url, .. } = auth {
                *token_url = url;
            }
        })
    };
    let drafts = [
        set_auth("file:///etc/passwd"),
        set_auth("myapp://authorize"),
        set_auth("javascript:alert(1)"),
        set_token("file:///etc/passwd"),
        set_token("myapp://token"),
    ];

    for resolved in &drafts {
        let flow_id = uuid::Uuid::new_v4().to_string();
        let authorize = app
            .oauth()
            .authorize(&flow_id, &app.location, resolved, &"0".repeat(64), &browser)
            .await;
        let status = app.oauth().status(&app.location, resolved).await;
        let send = app.attach(resolved).await;

        assert_eq!(authorize.unwrap_err().code, ErrorCode::InvalidInput);
        assert_eq!(status.unwrap_err().code, ErrorCode::InvalidInput);
        assert_eq!(send.unwrap_err().code, ErrorCode::InvalidInput);
    }
    assert!(browser.opened.lock().unwrap().is_empty());
    assert!(idp.hits().is_empty());
}

#[tokio::test]
async fn an_unresolved_variable_is_invalid_input_naming_it() {
    let idp = idp::spawn(IdpConfig::default()).await;
    let app = App::new();
    let browser = FakeBrowser::default();
    let unresolved = Resolved {
        unresolved: vec!["client".to_string()],
        ..draft_with(&idp, CODE, |auth| {
            if let Auth::Oauth2 { client_id, .. } = auth {
                *client_id = "{{client}}".to_string();
            }
        })
    };

    let authorize = app
        .oauth()
        .authorize(
            &uuid::Uuid::new_v4().to_string(),
            &app.location,
            &unresolved,
            &"0".repeat(64),
            &browser,
        )
        .await
        .unwrap_err();
    let send = app.attach(&unresolved).await.unwrap_err();

    for error in [authorize, send] {
        assert_eq!(error.code, ErrorCode::InvalidInput);
        assert!(error.message.contains("`client`"));
    }
    assert!(browser.opened.lock().unwrap().is_empty() && idp.hits().is_empty());
}

#[tokio::test]
async fn the_21st_record_evicts_the_oldest() {
    let idp = idp::spawn(IdpConfig::default()).await;
    let app = App::new();
    let drafts: Vec<Resolved> = (1..=21)
        .map(|n| {
            draft_with(&idp, CREDENTIALS, |auth| {
                set_scope(auth, &format!("scope-{n}"))
            })
        })
        .collect();

    for resolved in &drafts {
        app.authorize(resolved, &FakeBrowser::default())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(2)).await;
    }

    assert_eq!(
        app.attach(&drafts[0]).await.unwrap_err().message,
        NO_TOKEN_MESSAGE
    );
    assert!(app.attach(&drafts[1]).await.is_ok());
    assert!(app.attach(&drafts[20]).await.is_ok());
    let index = app
        .vault
        .get(&format!("o/{COLLECTION}/index"))
        .unwrap()
        .unwrap();
    let index: serde_json::Value = serde_json::from_str(&index).unwrap();
    assert_eq!(index.as_array().unwrap().len(), 20);
}
