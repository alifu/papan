//! The watcher reports outside changes once and ignores our own writes (plan §5, §9).

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use papan_lib::store::watcher::{self, CollectionWatcher};
use papan_lib::store::Store;

const EVENT_TIMEOUT: Duration = Duration::from_secs(5);
const QUIET: Duration = Duration::from_secs(1);

struct Watched {
    _temp: tempfile::TempDir,
    root: PathBuf,
    id: String,
    store: Arc<Mutex<Store>>,
    events: Receiver<String>,
    _watcher: CollectionWatcher,
}

fn watched() -> Watched {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::new(&temp.path().join("app")).unwrap();
    let tree = store.create_collection(temp.path(), "W").unwrap();
    let id = tree.collection.id;
    store.create_request(&id, "", "Existing").unwrap();
    let root = PathBuf::from(tree.collection.path);
    let store = Arc::new(Mutex::new(store));
    let (sender, events) = channel();
    let on_change = move |id: &str| {
        let _ = sender.send(id.to_string());
    };
    let handle = watcher::watch(&root, id.clone(), store.clone(), on_change).unwrap();
    Watched {
        _temp: temp,
        root,
        id,
        store,
        events,
        _watcher: handle,
    }
}

impl Watched {
    fn expect_one_event(&self, what: &str) {
        let got = self
            .events
            .recv_timeout(EVENT_TIMEOUT)
            .unwrap_or_else(|_| panic!("no event after {what}"));
        assert_eq!(got, self.id);
        assert!(
            self.events.recv_timeout(QUIET).is_err(),
            "more than one event after {what}"
        );
    }

    fn expect_no_event(&self, what: &str) {
        assert!(
            self.events.recv_timeout(QUIET).is_err(),
            "unexpected event after {what}"
        );
    }
}

#[test]
fn outside_changes_produce_one_event_each() {
    let w = watched();
    let requests = w.root.join("requests");
    let created = requests.join("outside.http.yaml");

    fs::write(&created, "kind: http\nmethod: GET\nname: Outside\n").unwrap();
    w.expect_one_event("create");

    fs::write(&created, "kind: http\nmethod: POST\nname: Outside\n").unwrap();
    w.expect_one_event("write");

    let renamed = requests.join("renamed.http.yaml");
    fs::rename(&created, &renamed).unwrap();
    w.expect_one_event("rename");

    fs::remove_file(&renamed).unwrap();
    w.expect_one_event("delete");
}

#[test]
fn own_writes_and_git_changes_produce_none() {
    let w = watched();

    w.store
        .lock()
        .unwrap()
        .create_request(&w.id, "", "Mine")
        .unwrap();
    w.expect_no_event("a store write");

    let git = w.root.join(".git");
    fs::create_dir_all(&git).unwrap();
    fs::write(git.join("index"), "x").unwrap();
    w.expect_no_event("a change under .git");
}
