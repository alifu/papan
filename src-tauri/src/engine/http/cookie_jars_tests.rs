use std::sync::mpsc;

use url::Url;

use super::*;
use crate::vars::types::{SecretStorageMode, SecretStorageStatus};
use crate::vars::vault::{MemoryVault, SecretChange};

const JAR: &str = "scratch";

#[test]
fn only_verified_jars_name_a_file() {
    let id = "7f3c2b1a-9d8e-4f60-a1b2-c3d4e5f60718";

    assert_eq!(file_name("scratch").as_deref(), Some("scratch"));
    assert_eq!(
        file_name(&format!("collection:{id}")),
        Some(format!("collection-{id}"))
    );
    assert_eq!(file_name("scratch#tls-unverified"), None);
    assert_eq!(file_name(&format!("collection:{id}#tls-unverified")), None);
}

fn jars(dir: &std::path::Path, vault: Arc<dyn SecretStore>) -> Arc<CookieJars> {
    let persisted = Persisted {
        dir: dir.to_path_buf(),
        vault,
    };
    Arc::new(CookieJars::new(Arc::default(), Some(persisted)))
}

fn add_persistent(store: &CookieStoreMutex, name: &str) {
    let url = Url::parse("https://api.example/").unwrap();
    lock_store(store)
        .parse(&format!("{name}=1; Max-Age=3600"), &url)
        .unwrap();
}

fn saved_file(dir: &std::path::Path, vault: &dyn SecretStore) -> Vec<u8> {
    let mut store = CookieStore::default();
    let url = Url::parse("https://api.example/").unwrap();
    store.parse("sid=1; Max-Age=3600", &url).unwrap();
    cookie_file::save(dir, JAR, vault, &store).unwrap();
    std::fs::read(dir.join("scratch.jar")).unwrap()
}

#[tokio::test]
async fn a_jar_whose_file_cannot_be_read_is_not_loaded_nor_saved_over_it() {
    let dir = tempfile::tempdir().unwrap();
    let before = saved_file(dir.path(), &MemoryVault::default());
    let denied = Arc::new(MemoryVault::new(SecretStorageMode::SessionOnly));
    let jars = jars(dir.path(), denied);

    let store = jars.open(JAR).await.unwrap();
    add_persistent(&store, "fresh");
    jars.save(JAR).await;

    assert!(!recover(&jars.loaded).contains(JAR));
    assert_eq!(
        std::fs::read(dir.path().join("scratch.jar")).unwrap(),
        before
    );
}

/// A keychain vault whose reads wait until the test lets them go.
struct Gated {
    inner: MemoryVault,
    entered: Mutex<mpsc::Sender<()>>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl SecretStore for Gated {
    fn get(&self, id: &str) -> Result<Option<String>, AppError> {
        recover(&self.entered).send(()).ok();
        recover(&self.release).recv().ok();
        self.inner.get(id)
    }
    fn has(&self, id: &str) -> Result<bool, AppError> {
        self.inner.has(id)
    }
    fn apply(&self, changes: &[SecretChange]) -> Result<(), AppError> {
        self.inner.apply(changes)
    }
    fn delete_prefix(&self, prefix: &str) -> Result<(), AppError> {
        self.inner.delete_prefix(prefix)
    }
    fn status(&self) -> SecretStorageStatus {
        self.inner.status()
    }
    fn reset(&self) -> Result<(), AppError> {
        self.inner.reset()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_clear_during_a_save_leaves_no_file_behind() {
    let dir = tempfile::tempdir().unwrap();
    let (entered_tx, entered) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let vault = Arc::new(Gated {
        inner: MemoryVault::default(),
        entered: Mutex::new(entered_tx),
        release: Mutex::new(release_rx),
    });
    let jars = jars(dir.path(), vault);
    add_persistent(&jars.clients.jar(JAR), "sid");
    recover(&jars.loaded).insert(JAR.to_string());

    let saving = tokio::spawn({
        let jars = Arc::clone(&jars);
        async move { jars.save(JAR).await }
    });
    entered.recv().unwrap();
    jars.clear(JAR);
    for _ in 0..4 {
        release.send(()).ok();
    }
    saving.await.unwrap();

    assert!(!dir.path().join("scratch.jar").exists());
}

fn scratch() -> RequestLocation {
    RequestLocation::Scratch { id: "s".into() }
}

#[tokio::test]
async fn a_domain_cookie_goes_to_subdomains_and_a_host_only_cookie_does_not() {
    let jars = Arc::new(CookieJars::new(Arc::default(), None));
    let store = jars.open(JAR).await.unwrap();
    let origin = Url::parse("https://example.com/").unwrap();
    lock_store(&store)
        .parse("wide=1; Domain=example.com; Path=/", &origin)
        .unwrap();
    lock_store(&store).parse("only=2; Path=/", &origin).unwrap();
    lock_store(&store)
        .parse("sec=3; Secure; Path=/", &origin)
        .unwrap();

    let sub_ws = Url::parse("ws://api.example.com/x").unwrap();
    let sub_wss = Url::parse("wss://api.example.com/x").unwrap();
    let apex_ws = Url::parse("ws://example.com/x").unwrap();

    let loc = scratch();
    let header = |url: Url| {
        let (jars, loc) = (Arc::clone(&jars), loc.clone());
        async move { jars.request_header(&loc, &url, true).await }
    };
    assert_eq!(header(sub_ws).await.unwrap().as_deref(), Some("wide=1"));
    assert_eq!(header(sub_wss).await.unwrap().as_deref(), Some("wide=1"));
    let apex = header(apex_ws).await.unwrap().unwrap();
    assert!(apex.contains("wide=1") && apex.contains("only=2") && !apex.contains("sec=3"));
}

#[tokio::test]
async fn the_unverified_jar_is_read_when_tls_is_not_verified() {
    let jars = Arc::new(CookieJars::new(Arc::default(), None));
    let url = Url::parse("https://example.com/").unwrap();
    lock_store(&jars.clients.jar("scratch#tls-unverified"))
        .parse("u=1; Path=/", &url)
        .unwrap();
    let ws = Url::parse("wss://example.com/").unwrap();

    assert_eq!(
        jars.request_header(&scratch(), &ws, false)
            .await
            .unwrap()
            .as_deref(),
        Some("u=1")
    );
    assert_eq!(
        jars.request_header(&scratch(), &ws, true).await.unwrap(),
        None
    );
}
