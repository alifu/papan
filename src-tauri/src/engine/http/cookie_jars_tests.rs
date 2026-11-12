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
