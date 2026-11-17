//! Cancel and Clear racing a token request: a hanging refresh, an early Cancel and a
//! Clear while a renewal is in flight.

use std::sync::Arc;
use std::time::Duration;

use papan_lib::engine::http::oauth::tokens::{self, Owner, TokenRecord};
use papan_lib::engine::http::resolve::Resolved;
use papan_lib::error::ErrorCode;
use papan_lib::store::types::Auth;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Notify;

use crate::app::{draft, draft_with, key_of, App, FakeBrowser};
use crate::support::idp::{self, IdpConfig};
use crate::CODE;

const RENEWED: &str =
    r#"{"access_token":"renewed-access","token_type":"Bearer","expires_in":3600}"#;
const LIMIT: Duration = Duration::from_secs(10);

/// A token endpoint that signals `accepted` per request and answers only once
/// `release` is notified.
struct SlowEndpoint {
    url: String,
    accepted: Arc<Notify>,
    release: Arc<Notify>,
}

async fn slow_endpoint() -> SlowEndpoint {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/token", listener.local_addr().unwrap());
    let (accepted, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    let (on_accept, on_release) = (Arc::clone(&accepted), Arc::clone(&release));
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let (on_accept, on_release) = (Arc::clone(&on_accept), Arc::clone(&on_release));
            tokio::spawn(async move {
                let mut buffer = [0u8; 8192];
                let _ = stream.read(&mut buffer).await;
                on_accept.notify_one();
                on_release.notified().await;
                let reply = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                     content-length: {}\r\nconnection: close\r\n\r\n{RENEWED}",
                    RENEWED.len()
                );
                let _ = stream.write_all(reply.as_bytes()).await;
            });
        }
    });
    SlowEndpoint {
        url,
        accepted,
        release,
    }
}

/// A code draft whose token URL is `endpoint`, with an expired record stored for it.
fn seeded(app: &App, resource: &idp::TestIdp, endpoint: &SlowEndpoint) -> Resolved {
    let resolved = draft_with(resource, CODE, |auth| {
        if let Auth::Oauth2 { token_url, .. } = auth {
            *token_url = endpoint.url.clone();
        }
    });
    let record = TokenRecord {
        access_token: "expired-access".to_string(),
        refresh_token: Some("stored-refresh".to_string()),
        expires_at_ms: Some(1),
        scope: "read write".to_string(),
        token_url: endpoint.url.clone(),
        saved_at_ms: 1,
        verify_tls: true,
    };
    let owner = Owner::of(&app.location).unwrap();
    tokens::save(&*app.vault, &owner, &key_of(&resolved), &record).unwrap();
    resolved
}

#[tokio::test]
async fn cancel_stops_a_send_whose_refresh_hangs() {
    let resource = idp::spawn(IdpConfig::default()).await;
    let endpoint = slow_endpoint().await;
    let app = App::new();
    let resolved = seeded(&app, &resource, &endpoint);
    let id = uuid::Uuid::new_v4().to_string();

    let sending = app.engine.send_resolved(&id, &app.location, &resolved);
    let cancelling = async {
        endpoint.accepted.notified().await;
        app.engine.cancel(&id).unwrap();
    };
    let both = async { tokio::join!(sending, cancelling) };
    let (sent, ()) = tokio::time::timeout(LIMIT, both).await.unwrap();

    assert_eq!(sent.unwrap_err().code, ErrorCode::Cancelled);
    assert_eq!(app.engine.in_flight(), 0);
}

#[tokio::test]
async fn clear_during_a_refresh_is_not_undone_by_its_save() {
    let resource = idp::spawn(IdpConfig::default()).await;
    let endpoint = slow_endpoint().await;
    let app = App::new();
    let resolved = seeded(&app, &resource, &endpoint);

    let clearing = async {
        endpoint.accepted.notified().await;
        let release = async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            endpoint.release.notify_one();
        };
        tokio::join!(app.oauth().clear(&app.location, &resolved), release).0
    };
    let both = async { tokio::join!(app.attach(&resolved), clearing) };
    let (attached, cleared) = tokio::time::timeout(LIMIT, both).await.unwrap();

    assert!(attached.is_ok());
    assert!(!cleared.unwrap().has_token);
    let owner = Owner::of(&app.location).unwrap();
    let stored = tokens::load(&*app.vault, &owner, &key_of(&resolved)).unwrap();
    assert!(stored.is_none());
}

#[tokio::test]
async fn a_cancel_while_resolving_ends_the_flow_before_the_browser_opens() {
    let idp = idp::spawn(IdpConfig::default()).await;
    let app = App::new();
    let resolved = draft(&idp, CODE);
    let flow_id = uuid::Uuid::new_v4().to_string();
    let browser = FakeBrowser::default();

    let flow = app.oauth().begin(&flow_id).unwrap();
    app.oauth().cancel(&flow_id).unwrap();
    let resolving = flow.unless_cancelled(async { Ok(resolved.clone()) }).await;
    let pending = app
        .oauth()
        .authorize_pending(
            &flow,
            &app.location,
            &resolved,
            &key_of(&resolved),
            &browser,
        )
        .await;

    assert_eq!(resolving.unwrap_err().code, ErrorCode::Cancelled);
    assert_eq!(pending.unwrap_err().code, ErrorCode::Cancelled);
    assert!(browser.opened.lock().unwrap().is_empty());
    assert!(idp.hits().is_empty());
}
