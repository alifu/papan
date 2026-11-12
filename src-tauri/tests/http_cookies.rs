//! Persisted cookie jars and the cookie manager through `HttpEngine` (plan 03b §6.5).

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use papan_lib::engine::http::types::{CookieFilter, CookieInput};
use papan_lib::engine::http::HttpEngine;
use papan_lib::store::types::{RequestContent, RequestLocation, Settings};
use papan_lib::vars::vault::{MemoryVault, SecretStore};
use support::server::{self, TestServer};

const VALUE: &str = "jarValue7731";
const COLLECTION: &str = "7f3c2b1a-9d8e-4f60-a1b2-c3d4e5f60718";
const FUTURE_MS: f64 = 4_102_444_800_000.0; // 2100-01-01

/// App data and a vault that outlive one engine, as across an app restart.
struct App {
    dir: tempfile::TempDir,
    vault: Arc<dyn SecretStore>,
}

impl App {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            vault: Arc::new(MemoryVault::default()),
        }
    }

    fn engine(&self) -> HttpEngine {
        let data = self.dir.path();
        HttpEngine::new(&data.join("cache"), data, "9.9.9")
            .unwrap()
            .with_vault(data, Arc::clone(&self.vault))
    }

    fn jar_files(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.dir.path().join("cookies"))
            .map(|dir| dir.map(|entry| entry.unwrap().path()).collect())
            .unwrap_or_default()
    }
}

fn collection() -> RequestLocation {
    RequestLocation::Collection {
        collection_id: COLLECTION.into(),
        path: "r.http.yaml".into(),
    }
}

fn scratch() -> RequestLocation {
    RequestLocation::Scratch { id: "s".into() }
}

fn get(url: &str) -> RequestContent {
    RequestContent::new("GET", url)
}

fn unverified(url: &str) -> RequestContent {
    RequestContent {
        settings: Settings {
            verify_tls: false,
            ..Settings::default()
        },
        ..get(url)
    }
}

/// The `Cookie` header the server received.
async fn sent_cookies(
    engine: &HttpEngine,
    location: &RequestLocation,
    content: &RequestContent,
) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    engine.send(&id, location, content).await.unwrap();
    engine.read_body(&id).unwrap().data
}

async fn set_from_server(engine: &HttpEngine, server: &TestServer, location: &RequestLocation) {
    let url = server.url(&format!("/cookies/set?sid={VALUE}"));
    sent_cookies(engine, location, &get(&url)).await;
}

fn files_containing(root: &Path, needle: &[u8]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in walk(root) {
        let bytes = std::fs::read(&entry).unwrap();
        if bytes.windows(needle.len()).any(|window| window == needle) {
            found.push(entry);
        }
    }
    found
}

fn walk(root: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(root)
        .unwrap()
        .flat_map(|entry| {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path)
            } else {
                vec![path]
            }
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn persistent_cookies_survive_a_new_engine_and_are_sealed_on_disk() {
    let server = server::spawn().await;
    let app = App::new();
    let first = app.engine();
    set_from_server(&first, &server, &collection()).await;
    first.cookies().flush().await;
    drop(first);

    let second = app.engine();
    let after_restart = sent_cookies(&second, &collection(), &get(&server.url("/cookies"))).await;
    let in_clear = files_containing(&app.dir.path().join("cookies"), VALUE.as_bytes());
    let files = app.jar_files();
    second.clear_cookies(&collection());

    assert_eq!(after_restart, format!("sid={VALUE}"));
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files[0].ends_with(format!("collection-{COLLECTION}.jar")));
    assert!(in_clear.is_empty(), "{in_clear:?}");
    assert!(app.jar_files().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_unverified_jar_is_never_written() {
    let server = server::spawn().await;
    let app = App::new();
    let engine = app.engine();
    let set = server.url(&format!("/cookies/set?u={VALUE}"));

    sent_cookies(&engine, &scratch(), &unverified(&set)).await;
    sent_cookies(&engine, &scratch(), &get(&server.url("/cookies"))).await;
    engine.cookies().flush().await;
    let same_mode = sent_cookies(&engine, &scratch(), &unverified(&server.url("/cookies"))).await;

    assert_eq!(same_mode, format!("u={VALUE}"));
    assert!(app.jar_files().is_empty(), "{:?}", app.jar_files());
}

#[tokio::test(flavor = "multi_thread")]
async fn set_and_delete_change_what_the_next_send_sends_and_are_saved() {
    let server = server::spawn().await;
    let app = App::new();
    let engine = app.engine();
    let read = get(&server.url("/cookies"));
    let host = url::Url::parse(&server.url("/")).unwrap();
    let cookie = CookieInput {
        name: "m".into(),
        value: VALUE.into(),
        domain: host.host_str().unwrap().into(),
        path: "/".into(),
        expires_at_ms: Some(FUTURE_MS),
        http_only: false,
        secure: false,
    };

    let listed = engine.cookies().set(&scratch(), &cookie).await.unwrap();
    let after_set = sent_cookies(&engine, &scratch(), &read).await;
    // The debounced save, without a flush.
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    let saved = app.jar_files();
    let filter = CookieFilter {
        domain: cookie.domain.clone(),
        path: None,
        name: None,
    };
    let remaining = engine.cookies().delete(&scratch(), &filter).await.unwrap();
    let after_delete = sent_cookies(&engine, &scratch(), &read).await;
    engine.cookies().flush().await;

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].value, VALUE);
    assert_eq!(after_set, format!("m={VALUE}"));
    assert_eq!(saved.len(), 1, "{saved:?}");
    assert!(saved[0].ends_with("scratch.jar"));
    assert!(remaining.is_empty());
    assert_eq!(after_delete, "");
    assert!(app.jar_files().is_empty());
    assert!(engine.cookies().list(&scratch()).await.unwrap().is_empty());
}
