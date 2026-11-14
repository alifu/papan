//! Plan 03 §4 and §7: secrets never reach a collection folder; history snapshots.

use std::fs;
use std::path::Path;

use papan_lib::error::ErrorCode;
use papan_lib::store::history::{Outcome, Sent};
use papan_lib::store::types::{Auth, KeyValue, RequestContent, RequestKind, RequestLocation};
use papan_lib::store::Store;

const TOKEN: &str = "tok-SECRET-1234";
const AUTH_VALUE: &str = "Basic c2VjcmV0LWhlYWRlcg==";

struct Fixture {
    _app: tempfile::TempDir,
    work: tempfile::TempDir,
    store: Store,
}

fn fixture() -> Fixture {
    let app = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let store = Store::new(app.path()).unwrap();
    Fixture {
        _app: app,
        work,
        store,
    }
}

fn secret_content() -> RequestContent {
    RequestContent {
        headers: vec![KeyValue {
            name: "Authorization".into(),
            value: AUTH_VALUE.into(),
            enabled: true,
        }],
        auth: Auth::Bearer {
            token: TOKEN.into(),
        },
        ..RequestContent::new("POST", "https://api.example.com")
    }
}

fn tree_contains(dir: &Path, needle: &str) -> bool {
    fs::read_dir(dir).unwrap().any(|entry| {
        let path = entry.unwrap().path();
        if path.is_dir() {
            tree_contains(&path, needle)
        } else {
            let bytes = fs::read(&path).unwrap();
            bytes.windows(needle.len()).any(|w| w == needle.as_bytes())
        }
    })
}

fn collection(f: &mut Fixture) -> (String, RequestLocation) {
    let id = f
        .store
        .create_collection(f.work.path(), "C")
        .unwrap()
        .collection
        .id;
    f.store
        .create_request(&id, "", "R", RequestKind::Http)
        .unwrap();
    let location = RequestLocation::Collection {
        collection_id: id.clone(),
        path: "r.http.yaml".into(),
    };
    (id, location)
}

#[test]
fn collection_secrets_stay_in_memory_and_out_of_the_folder() {
    let mut f = fixture();
    let (_, location) = collection(&mut f);

    f.store.write_request(&location, &secret_content()).unwrap();
    let read = f.store.read_request(&location).unwrap();

    assert_eq!(read.content, secret_content());
    for needle in [TOKEN, AUTH_VALUE] {
        assert!(!tree_contains(f.work.path(), needle), "{needle} on disk");
    }
}

#[test]
fn an_outside_edit_drops_the_remembered_secrets() {
    let mut f = fixture();
    let (_, location) = collection(&mut f);
    f.store.write_request(&location, &secret_content()).unwrap();
    let file = f.work.path().join("c/requests/r.http.yaml");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, text.replace("api.example.com", "other.example.com")).unwrap();

    let read = f.store.read_request(&location).unwrap();

    assert_eq!(
        read.content.auth,
        Auth::Bearer {
            token: String::new()
        }
    );
    assert_eq!(read.content.url, "https://other.example.com");
}

#[test]
fn scratch_keeps_secrets_on_disk_and_moving_it_redacts_them() {
    let mut f = fixture();
    let (id, _) = collection(&mut f);
    let scratch = f.store.create_scratch(RequestKind::Http).unwrap();
    let in_scratch = RequestLocation::Scratch {
        id: scratch.id.clone(),
    };
    f.store
        .write_request(&in_scratch, &secret_content())
        .unwrap();
    let kept = f.store.read_request(&in_scratch).unwrap();

    let moved = f
        .store
        .move_scratch_to_collection(&scratch.id, &id, "")
        .unwrap();
    let location = RequestLocation::Collection {
        collection_id: id,
        path: moved.path.unwrap(),
    };

    assert_eq!(kept.content, secret_content());
    assert!(!tree_contains(f.work.path(), TOKEN));
    assert_eq!(
        f.store.read_request(&location).unwrap().content,
        secret_content()
    );
}

#[test]
fn history_restores_a_redacted_snapshot_into_scratch() {
    let mut f = fixture();
    let (_, location) = collection(&mut f);
    let name = f.store.request_name(&location).unwrap();
    f.store
        .append_history(&Sent {
            location: &location,
            name: &name,
            content: &secret_content(),
            sent_at_ms: 1_700_000_000_000.0,
            outcome: Outcome::Failed {
                code: ErrorCode::Network,
                total_ms: 3,
            },
        })
        .unwrap();
    let entry = f.store.list_history().remove(0);

    let restored = f.store.restore_history_entry(&entry.id).unwrap();
    let doc = f
        .store
        .read_request(&RequestLocation::Scratch {
            id: restored.id.clone(),
        })
        .unwrap();
    f.store.clear_history().unwrap();

    assert_eq!((entry.name.as_str(), restored.name.as_str()), ("R", "R"));
    assert_eq!(entry.error_code, Some(ErrorCode::Network));
    assert_eq!(
        doc.content.auth,
        Auth::Bearer {
            token: String::new()
        }
    );
    assert!(f.store.list_history().is_empty());
}
