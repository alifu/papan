//! The loopback listener as a local attacker sees it, flow lifetime, and the leak check.

use std::sync::Arc;
use std::time::Duration;

use papan_lib::engine::http::client::Clients;
use papan_lib::engine::http::oauth::flow::NO_TOKEN_MESSAGE;
use papan_lib::engine::http::oauth::OAuth;
use papan_lib::engine::http::service::record_history;
use papan_lib::error::ErrorCode;
use papan_lib::store::types::{Auth, OAuthGrant};
use papan_lib::store::{Store, StoreState};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use url::Url;

use crate::app::{self, draft, draft_with, key_of, query, App, FakeBrowser, ManualBrowser};
use crate::support::idp::{self, IdpConfig, CLIENT_SECRET};

const CODE: OAuthGrant = OAuthGrant::AuthorizationCode;

/// One raw HTTP request to the listener; the whole reply.
async fn raw(port: u16, target: &str, host: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let request = format!("GET {target} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).await.unwrap();
    reply
}

fn manual() -> (ManualBrowser, tokio::sync::mpsc::UnboundedReceiver<Url>) {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    (ManualBrowser(sender), receiver)
}

fn port_of(authorize_url: &Url) -> u16 {
    Url::parse(&query(authorize_url, "redirect_uri"))
        .unwrap()
        .port()
        .unwrap()
}

async fn closed(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).await.is_err()
}

#[tokio::test]
async fn forged_callbacks_get_an_error_page_and_the_flow_keeps_waiting() {
    let idp = idp::spawn(IdpConfig::default()).await;
    let app = App::new();
    let resolved = draft(&idp, CODE);
    let (browser, mut opened) = manual();

    let flow = app.authorize(&resolved, &browser);
    let attacker = async {
        let url = opened.recv().await.unwrap();
        let port = port_of(&url);
        let host = format!("127.0.0.1:{port}");
        let state = query(&url, "state");
        let good_query = format!("/callback?code=forged-code-1&state={state}");

        let wrong_state = raw(
            port,
            "/callback?code=forged-code-2&state=guessed-state",
            &host,
        )
        .await;
        let no_state = raw(port, "/callback?error=access_denied", &host).await;
        let rebinding = raw(port, &good_query, "evil.example.test").await;
        let localhost = raw(port, &good_query, &format!("localhost:{port}")).await;
        let other_path = raw(port, &format!("/other?state={state}&q=marker-text"), &host).await;

        assert!(wrong_state.starts_with("HTTP/1.1 400 "), "{wrong_state}");
        assert!(no_state.starts_with("HTTP/1.1 400 "), "{no_state}");
        for reply in [&rebinding, &localhost, &other_path] {
            assert!(reply.starts_with("HTTP/1.1 404 "), "{reply}");
        }
        for reply in [&wrong_state, &no_state, &rebinding, &localhost, &other_path] {
            assert!(reply.contains("Cache-Control: no-store"));
            assert!(reply.contains("Referrer-Policy: no-referrer"));
            assert!(reply.contains("Content-Security-Policy: default-src 'none'"));
            for echoed in [
                "forged-code",
                "guessed-state",
                "marker-text",
                "evil.example",
                &state,
            ] {
                assert!(!reply.contains(echoed), "{echoed} echoed in {reply}");
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            app.oauth().pending(),
            1,
            "the flow ended on a forged request"
        );
        assert!(idp.token_hits().is_empty());

        // The real browser round trip still completes the flow.
        let page = reqwest::get(url.clone())
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(page.contains("You can close this tab."));
        assert!(!page.contains(&state));
        port
    };
    let (status, port) = tokio::join!(flow, attacker);

    assert!(status.unwrap().has_token);
    assert!(closed(port).await);
    assert_eq!(idp.token_hits().len(), 1);
}

#[tokio::test]
async fn cancel_ends_the_flow_and_closes_the_listener() {
    let idp = idp::spawn(IdpConfig::default()).await;
    let app = App::new();
    let resolved = draft(&idp, CODE);
    let (browser, mut opened) = manual();
    let flow_id = uuid::Uuid::new_v4().to_string();

    let key = key_of(&resolved);
    let flow = app
        .oauth()
        .authorize(&flow_id, &app.location, &resolved, &key, &browser);
    let cancel = async {
        let url = opened.recv().await.unwrap();
        let port = port_of(&url);
        assert!(!closed(port).await);
        app.oauth().cancel(&flow_id).unwrap();
        port
    };
    let (result, port) = tokio::join!(flow, cancel);

    assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
    assert!(closed(port).await);
    assert_eq!(app.oauth().pending(), 0);
    assert!(idp.hits().is_empty());
}

#[tokio::test]
async fn the_flow_times_out_and_closes_the_listener() {
    let idp = idp::spawn(IdpConfig::default()).await;
    let app = App::new();
    let oauth = OAuth::new(Arc::new(Clients::default()), app.engine.network())
        .with_lifetime(Duration::from_millis(200));
    let resolved = draft(&idp, CODE);
    let (browser, mut opened) = manual();
    let flow_id = uuid::Uuid::new_v4().to_string();

    let result = oauth
        .authorize(
            &flow_id,
            &app.location,
            &resolved,
            &key_of(&resolved),
            &browser,
        )
        .await;

    assert_eq!(result.unwrap_err().code, ErrorCode::Timeout);
    let port = port_of(&opened.recv().await.unwrap());
    assert!(closed(port).await);
    assert_eq!(oauth.pending(), 0);
}

#[tokio::test]
async fn a_busy_fixed_port_is_a_conflict_naming_the_port() {
    let idp = idp::spawn(IdpConfig::default()).await;
    let app = App::new();
    let busy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = busy.local_addr().unwrap().port();
    let resolved = draft_with(&idp, CODE, |auth| {
        if let Auth::Oauth2 { redirect_port, .. } = auth {
            *redirect_port = port;
        }
    });
    let browser = FakeBrowser::default();

    let error = app.authorize(&resolved, &browser).await.unwrap_err();
    let status = app.oauth().status(&app.location, &resolved).await.unwrap();

    assert!(error.message.contains(&port.to_string()));
    assert!(browser.opened.lock().unwrap().is_empty());
    assert_eq!(
        status.redirect_uri.as_deref(),
        Some(format!("http://127.0.0.1:{port}/callback").as_str())
    );
}

#[tokio::test]
async fn no_file_log_history_or_returned_value_holds_a_token_code_verifier_or_secret() {
    let idp = idp::spawn(IdpConfig {
        token_lifetime: Some(Duration::from_secs(1)),
        ..IdpConfig::default()
    })
    .await;
    let app = App::new();
    let resolved = draft(&idp, CODE);
    let store = StoreState::new(Store::new(&app.dir.path().join("data")).unwrap());
    let mut returned: Vec<String> = Vec::new();

    let before = app.oauth().status(&app.location, &resolved).await.unwrap();
    let authorized = app
        .authorize(&resolved, &FakeBrowser::default())
        .await
        .unwrap();
    let sent = app.send(&resolved).await;
    record_history(
        &store,
        &app.location,
        &resolved.content,
        0.0,
        Duration::from_millis(5),
        &sent,
    );
    let meta = sent.unwrap();
    idp.configure(|config| config.token_error = Some("invalid_grant".to_string()));
    let failed = app.send(&resolved).await.unwrap_err();
    let after = app.oauth().status(&app.location, &resolved).await.unwrap();
    for status in [&before, &authorized, &after] {
        returned.push(serde_json::to_string(status).unwrap());
    }
    returned.push(serde_json::to_string(&meta).unwrap());
    returned.push(format!(
        "{failed:?} {}",
        serde_json::to_string(&failed).unwrap()
    ));
    returned.push(serde_json::to_string(&store.lock_store().list_history()).unwrap());

    let hits = idp.hits();
    let exchange = hits
        .iter()
        .find(|hit| hit.param("grant_type") == Some("authorization_code"))
        .unwrap();
    let authorize = hits.iter().find(|hit| hit.path == "/authorize").unwrap();
    let mut secrets = idp.issued();
    secrets.push(exchange.param("code_verifier").unwrap().to_string());
    secrets.push(authorize.param("state").unwrap().to_string());
    secrets.push(CLIENT_SECRET.to_string());
    assert!(secrets.len() >= 8, "{}", secrets.len());
    assert_eq!(meta.status, 200);
    // A rejected refresh forgets the record; the provider text only reaches the log.
    assert_eq!(failed.message, NO_TOKEN_MESSAGE);
    let logs = app::logs();
    assert!(logs.contains("invalid_grant"));
    assert!(
        logs.contains("requesting an OAuth token"),
        "nothing captured"
    );
    let files = app.files();
    assert!(files
        .iter()
        .any(|(path, _)| path.ends_with("secrets.vault")));
    for secret in &secrets {
        assert!(!logs.contains(secret), "a log line holds {secret}");
        for value in &returned {
            assert!(!value.contains(secret), "{value} holds {secret}");
        }
        for (path, bytes) in &files {
            let found = bytes
                .windows(secret.len())
                .any(|window| window == secret.as_bytes());
            assert!(!found, "{path} holds {secret}");
        }
    }
}
