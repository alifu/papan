//! Renewal at send: single flight and rotation, the token URL binding, the TLS setting
//! a token was issued under, and a provider that turns the renewal down for good.

use std::time::Duration;

use papan_lib::engine::http::oauth::flow::NO_TOKEN_MESSAGE;
use papan_lib::engine::http::resolve::Resolved;
use papan_lib::store::types::Auth;

use crate::app::{draft, draft_with, key_of, App, FakeBrowser, COLLECTION};
use crate::support::idp::{self, IdpConfig};
use crate::{short_lived, CODE, CREDENTIALS};

/// `resolved` sent without certificate verification.
fn insecure(resolved: &Resolved) -> Resolved {
    let mut content = resolved.content.clone();
    content.settings.verify_tls = false;
    Resolved {
        content,
        ..resolved.clone()
    }
}

#[tokio::test]
async fn an_expired_token_is_refreshed_once_under_ten_concurrent_sends_and_rotation_is_stored() {
    let idp = idp::spawn(short_lived()).await;
    let app = App::new();
    let resolved = draft(&idp, CODE);
    app.authorize(&resolved, &FakeBrowser::default())
        .await
        .unwrap();
    let first_refresh = idp.issued()[2].clone();
    idp.configure(|config| config.token_lifetime = Some(Duration::from_secs(3600)));

    let sends = (0..10).map(|_| app.send(&resolved));
    let results = futures_util::future::join_all(sends).await;

    for result in results {
        assert_eq!(result.unwrap().status, 200);
    }
    let refreshes: Vec<_> = idp
        .token_hits()
        .into_iter()
        .filter(|hit| hit.param("grant_type") == Some("refresh_token"))
        .collect();
    assert_eq!(refreshes.len(), 1);
    assert_eq!(
        refreshes[0].param("refresh_token"),
        Some(first_refresh.as_str())
    );

    // The rotated refresh token was stored: the next expiry refreshes with it.
    idp.configure(|config| config.token_lifetime = Some(Duration::from_secs(1)));
    let status = app.oauth().status(&app.location, &resolved).await.unwrap();
    assert!(status.can_refresh);
    let rotated = idp.issued().last().unwrap().clone();
    assert_ne!(rotated, first_refresh);
    let record_key = format!("o/{COLLECTION}/{}", key_of(&resolved));
    let stored = app.vault.get(&record_key).unwrap().unwrap();
    assert!(stored.contains(&rotated) && !stored.contains(&first_refresh));
}

#[tokio::test]
async fn after_the_token_url_changes_the_old_refresh_token_is_sent_nowhere() {
    let idp = idp::spawn(short_lived()).await;
    let other = idp::spawn(IdpConfig::default()).await;
    let app = App::new();
    let resolved = draft(&idp, CODE);
    app.authorize(&resolved, &FakeBrowser::default())
        .await
        .unwrap();
    let before = idp.hits().len();
    let moved = draft_with(&idp, CODE, |auth| {
        if let Auth::Oauth2 { token_url, .. } = auth {
            *token_url = other.token_url();
        }
    });

    let error = app.send(&moved).await.unwrap_err();

    assert_eq!(error.message, NO_TOKEN_MESSAGE);
    assert!(other.hits().is_empty());
    assert_eq!(idp.hits().len(), before);
    assert_ne!(key_of(&moved), key_of(&resolved));
}

#[tokio::test]
async fn a_renewal_with_weaker_tls_than_at_issue_is_refused_without_calling_the_provider() {
    for grant in [CODE, CREDENTIALS] {
        let idp = idp::spawn(short_lived()).await;
        let app = App::new();
        let resolved = draft(&idp, grant);
        app.authorize(&resolved, &FakeBrowser::default())
            .await
            .unwrap();
        let before = idp.token_hits().len();

        let error = app.attach(&insecure(&resolved)).await.unwrap_err();

        assert_eq!(error.message, NO_TOKEN_MESSAGE);
        assert_eq!(idp.token_hits().len(), before);
        let status = app.oauth().status(&app.location, &resolved).await.unwrap();
        assert!(status.has_token);
    }
}

#[tokio::test]
async fn a_renewal_with_the_same_or_stronger_tls_is_sent() {
    for grant in [CODE, CREDENTIALS] {
        let idp = idp::spawn(short_lived()).await;
        let app = App::new();
        let resolved = insecure(&draft(&idp, grant));
        app.authorize(&resolved, &FakeBrowser::default())
            .await
            .unwrap();
        let before = idp.token_hits().len();

        app.attach(&resolved).await.unwrap();
        app.attach(&draft(&idp, grant)).await.unwrap();

        assert_eq!(idp.token_hits().len(), before + 2);
        // Renewed with verification on, the token still counts as issued without it.
        let renewed = app.attach(&insecure(&resolved)).await;
        assert!(renewed.is_ok());
    }
}

#[tokio::test]
async fn status_reports_the_tls_setting_get_new_token_would_use() {
    let idp = idp::spawn(IdpConfig::default()).await;
    let app = App::new();
    let resolved = draft(&idp, CODE);

    let secure = app.oauth().status(&app.location, &resolved).await.unwrap();
    let weakened = insecure(&resolved);
    let weak = app.oauth().status(&app.location, &weakened).await.unwrap();

    assert!(secure.verify_tls);
    assert!(!weak.verify_tls);
}

#[tokio::test]
async fn a_rejected_refresh_forgets_the_record_and_asks_for_a_new_token() {
    let idp = idp::spawn(short_lived()).await;
    let app = App::new();
    let resolved = draft(&idp, CODE);
    app.authorize(&resolved, &FakeBrowser::default())
        .await
        .unwrap();
    idp.configure(|config| config.token_error = Some("invalid_grant".to_string()));
    let before = idp.token_hits().len();

    let error = app.attach(&resolved).await.unwrap_err();
    let again = app.attach(&resolved).await.unwrap_err();

    assert_eq!(error.message, NO_TOKEN_MESSAGE);
    assert_eq!(again.message, NO_TOKEN_MESSAGE);
    assert_eq!(idp.token_hits().len(), before + 1);
    let status = app.oauth().status(&app.location, &resolved).await.unwrap();
    assert!(!status.has_token);
}
