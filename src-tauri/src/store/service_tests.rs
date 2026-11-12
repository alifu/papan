use super::*;
use crate::error::ErrorCode;
use crate::store::types::NodeKind;

struct Fixture {
    _app: tempfile::TempDir,
    work: tempfile::TempDir,
    store: Store,
}

fn fixture() -> Fixture {
    let app = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let store = Store::new(&app.path().join("data")).unwrap();
    Fixture {
        _app: app,
        work,
        store,
    }
}

fn collection_location(id: &str, path: &str) -> RequestLocation {
    RequestLocation::Collection {
        collection_id: id.to_string(),
        path: path.to_string(),
    }
}

#[test]
fn create_open_close_and_reopen() {
    let mut f = fixture();

    let created = f.store.create_collection(f.work.path(), "My API").unwrap();
    let id = created.collection.id.clone();
    let reopened = f
        .store
        .open_collection(&f.work.path().join("my-api"))
        .unwrap();
    let listed = f.store.list_collections();
    f.store.close_collection(&id).unwrap();

    assert!(created.collection.available);
    assert_eq!(created.collection.name, "My API");
    assert_eq!(reopened.collection.id, id);
    assert_eq!(listed.len(), 1);
    assert!(f.store.list_collections().is_empty());
    assert_eq!(
        f.store.read_tree(&id).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert!(f
        .work
        .path()
        .join("my-api")
        .join("papan.collection.yaml")
        .exists());
}

#[test]
fn registry_survives_a_new_store() {
    let mut f = fixture();
    let created = f.store.create_collection(f.work.path(), "A").unwrap();

    let mut again = Store::new(&f._app.path().join("data")).unwrap();

    assert_eq!(
        again.list_collections()[0].collection.id,
        created.collection.id
    );
}

#[test]
fn missing_folder_is_unavailable_with_no_children() {
    let mut f = fixture();
    let created = f.store.create_collection(f.work.path(), "Gone").unwrap();
    let id = created.collection.id;
    f.store.create_request(&id, "", "R").unwrap();
    fsio::remove_dir_all(&f.work.path().join("gone")).unwrap();

    let tree = f.store.read_tree(&id).unwrap();

    assert!(!tree.collection.available);
    assert_eq!(tree.collection.name, "gone");
    assert!(tree.children.is_empty());
    assert!(!f.store.list_collections()[0].collection.available);
    assert!(f.store.watch_targets().is_empty());
}

#[test]
fn tree_operations_return_the_new_tree_and_path() {
    let mut f = fixture();
    let id = f
        .store
        .create_collection(f.work.path(), "C")
        .unwrap()
        .collection
        .id;

    let folder = f.store.create_folder(&id, "", "F").unwrap();
    let request = f.store.create_request(&id, "f", "R").unwrap();
    let renamed = f.store.rename_node(&id, "f/r.http.yaml", "S").unwrap();
    let moved = f.store.move_node(&id, "f/s.http.yaml", "", 0).unwrap();
    let deleted = f.store.delete_node(&id, "f").unwrap();

    assert_eq!(folder.path.as_deref(), Some("f"));
    assert_eq!(request.path.as_deref(), Some("f/r.http.yaml"));
    assert_eq!(renamed.path.as_deref(), Some("f/s.http.yaml"));
    assert_eq!(moved.path.as_deref(), Some("s.http.yaml"));
    assert_eq!(deleted.path, None);
    assert_eq!(deleted.tree.children.len(), 1);
    assert_eq!(deleted.tree.children[0].kind, NodeKind::Request);
}

#[test]
fn read_and_write_requests_by_location() {
    let mut f = fixture();
    let id = f
        .store
        .create_collection(f.work.path(), "C")
        .unwrap()
        .collection
        .id;
    f.store.create_request(&id, "", "R").unwrap();
    let scratch = f.store.create_scratch().unwrap();
    let content = RequestContent {
        method: "PUT".into(),
        url: "https://x".into(),
        ..RequestContent::default()
    };
    let in_collection = collection_location(&id, "r.http.yaml");
    let in_scratch = RequestLocation::Scratch {
        id: scratch.id.clone(),
    };

    f.store.write_request(&in_collection, &content).unwrap();
    f.store.write_request(&in_scratch, &content).unwrap();

    for location in [in_collection, in_scratch] {
        let doc = f.store.read_request(&location).unwrap();
        assert_eq!(
            (doc.content.method.as_str(), doc.content.url.as_str()),
            ("PUT", "https://x")
        );
    }
}

#[test]
fn scratch_crud_through_the_store() {
    let f = fixture();

    let item = f.store.create_scratch().unwrap();
    let renamed = f.store.rename_scratch(&item.id, "Named").unwrap();
    let listed = f.store.list_scratch().unwrap();
    let deleted = f.store.delete_scratch(&item.id).unwrap();

    assert_eq!(renamed[0].name, "Named");
    assert_eq!(listed, renamed);
    assert!(deleted.is_empty());
}

#[test]
fn move_scratch_writes_then_removes() {
    let mut f = fixture();
    let id = f
        .store
        .create_collection(f.work.path(), "C")
        .unwrap()
        .collection
        .id;
    let item = f.store.create_scratch().unwrap();

    let change = f
        .store
        .move_scratch_to_collection(&item.id, &id, "")
        .unwrap();

    assert_eq!(change.path.as_deref(), Some("untitled.http.yaml"));
    assert!(f.store.list_scratch().unwrap().is_empty());
    let doc = f
        .store
        .read_request(&collection_location(&id, "untitled.http.yaml"))
        .unwrap();
    assert_eq!(doc.name, "Untitled");
}

#[test]
fn move_scratch_keeps_the_scratch_file_when_the_write_fails() {
    let mut f = fixture();
    let id = f
        .store
        .create_collection(f.work.path(), "C")
        .unwrap()
        .collection
        .id;
    let item = f.store.create_scratch().unwrap();

    let error = f
        .store
        .move_scratch_to_collection(&item.id, &id, "missing")
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(f.store.list_scratch().unwrap(), vec![item]);
}

#[test]
fn fingerprint_ignores_own_writes_and_sees_outside_ones() {
    let mut f = fixture();
    let id = f
        .store
        .create_collection(f.work.path(), "C")
        .unwrap()
        .collection
        .id;
    f.store.create_request(&id, "", "R").unwrap();

    let after_own = f.store.refresh_fingerprint(&id);
    fsio::write_atomic(
        &f.work.path().join("c/requests/x.http.yaml"),
        b"kind: http\nmethod: GET\nname: X\n",
    )
    .unwrap();
    let after_outside = f.store.refresh_fingerprint(&id);
    let settled = f.store.refresh_fingerprint(&id);

    assert!(!after_own);
    assert!(after_outside);
    assert!(!settled);
}

#[test]
fn fingerprint_reports_a_disappearing_collection_once() {
    let mut f = fixture();
    let id = f
        .store
        .create_collection(f.work.path(), "C")
        .unwrap()
        .collection
        .id;
    fsio::remove_dir_all(&f.work.path().join("c")).unwrap();

    assert!(f.store.refresh_fingerprint(&id));
    assert!(!f.store.refresh_fingerprint(&id));
}

#[test]
fn open_rejects_folders_that_are_not_collections() {
    let mut f = fixture();

    let error = f.store.open_collection(f.work.path()).unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
}

#[test]
fn opening_sweeps_stale_temp_files() {
    let mut f = fixture();
    f.store.create_collection(f.work.path(), "C").unwrap();
    let stale = f.work.path().join("c/requests/.papan-tmp-old");
    fsio::write_atomic(&stale, b"x").unwrap();

    f.store.open_collection(&f.work.path().join("c")).unwrap();

    assert_eq!(fsio::entry_kind(&stale), None);
}
