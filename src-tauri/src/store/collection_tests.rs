use super::*;
use crate::error::ErrorCode;
use crate::store::scan::arrange;
use crate::store::types::{NodeKind, RequestContent, TreeNode};

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        fsio::create_dir_all(parent).unwrap();
    }
    fsio::write_atomic(path, text.as_bytes()).unwrap();
}

fn read(path: &Path) -> String {
    String::from_utf8(fsio::read_bounded(path).unwrap()).unwrap()
}

fn new_collection() -> (tempfile::TempDir, Collection) {
    let dir = tempfile::tempdir().unwrap();
    let collection = Collection::create(dir.path(), "My API").unwrap();
    (dir, collection)
}

fn names(nodes: &[TreeNode]) -> Vec<&str> {
    nodes.iter().map(|node| node.path.as_str()).collect()
}

fn root_order(collection: &Collection) -> Option<String> {
    let path = collection.requests_root().join(FOLDER_FILE);
    fsio::entry_kind(&path).map(|_| read(&path))
}

#[test]
fn create_writes_collection_file_and_requests_folder() {
    let (dir, collection) = new_collection();

    assert_eq!(collection.root, dir.path().join("my-api"));
    assert_eq!(collection.name, "My API");
    assert_eq!(
        read(&collection.root.join(COLLECTION_FILE)),
        "name: My API\nversion: 1\n"
    );
    assert_eq!(
        fsio::entry_kind(&collection.requests_root()),
        Some(EntryKind::Dir)
    );
    assert_eq!(Collection::open(&collection.root).unwrap(), collection);
}

#[test]
fn create_avoids_reserved_windows_names() {
    let dir = tempfile::tempdir().unwrap();

    let collection = Collection::create(dir.path(), "CON").unwrap();

    assert_eq!(collection.root, dir.path().join("con-2"));
}

#[test]
fn sweep_removes_stale_temp_files_everywhere() {
    let (_dir, collection) = new_collection();
    let folder = collection.create_folder("", "F").unwrap();
    write(&collection.root.join(".papan-tmp-1"), "x");
    write(
        &collection
            .requests_root()
            .join(&folder)
            .join(".papan-tmp-2"),
        "x",
    );

    assert_eq!(collection.sweep_temp_files(), 2);
    assert_eq!(collection.sweep_temp_files(), 0);
}

#[test]
fn new_names_never_land_on_an_existing_entry() {
    // KELVIN SIGN folds to `k` on case-insensitive filesystems (APFS, NTFS) but not in
    // the ASCII sibling check, so only the destination check prevents an overwrite.
    let (_dir, collection) = new_collection();
    let kelvin = collection.requests_root().join("\u{212A}.http.yaml");
    write(&kelvin, "kind: http\nmethod: GET\nname: Kelvin\n");
    let folds = fsio::entry_kind(&collection.requests_root().join("k.http.yaml")).is_some();

    let created = collection.create_request("", "K");

    assert_eq!(read(&kelvin), "kind: http\nmethod: GET\nname: Kelvin\n");
    if folds {
        assert_eq!(created.unwrap_err().code, ErrorCode::InvalidInput);
    } else {
        assert_eq!(created.unwrap(), "k.http.yaml");
    }
}

#[test]
fn create_refuses_an_existing_folder_and_a_missing_parent() {
    let (dir, _collection) = new_collection();

    let exists = Collection::create(dir.path(), "my api").unwrap_err();
    let missing = Collection::create(&dir.path().join("nope"), "x").unwrap_err();

    assert_eq!(exists.code, ErrorCode::InvalidInput);
    assert_eq!(missing.code, ErrorCode::NotFound);
}

#[test]
fn open_refuses_newer_version_and_non_collections() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join(COLLECTION_FILE), "name: X\nversion: 2\n");

    let newer = Collection::open(dir.path()).unwrap_err();
    let plain = Collection::open(&dir.path().join("x")).unwrap_err();

    assert_eq!(newer.message, "This collection was made by a newer Papan.");
    assert_eq!(plain.code, ErrorCode::InvalidInput);
}

#[test]
fn open_refuses_requests_that_is_a_file() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join(COLLECTION_FILE), "name: X\nversion: 1\n");
    write(&dir.path().join(REQUESTS_DIR), "not a dir");

    assert!(Collection::open(dir.path()).is_err());
}

#[test]
fn tree_of_collection_without_requests_folder_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join(COLLECTION_FILE), "name: X\nversion: 1\n");

    let collection = Collection::open(dir.path()).unwrap();

    assert!(collection.tree().is_empty());
}

#[test]
fn create_items_do_not_touch_order_and_sort_into_the_tail() {
    let (_dir, collection) = new_collection();

    let b = collection.create_request("", "B").unwrap();
    let a = collection.create_folder("", "A").unwrap();
    let nested = collection.create_request(&a, "Inner").unwrap();

    assert_eq!((a.as_str(), b.as_str()), ("a", "b.http.yaml"));
    assert_eq!(nested, "a/inner.http.yaml");
    assert_eq!(root_order(&collection), None);
    let tree = collection.tree();
    assert_eq!(names(&tree), ["a", "b.http.yaml"]);
    assert_eq!(tree[0].name, "A");
    assert_eq!(tree[0].kind, NodeKind::Folder);
    assert_eq!(tree[1].method.as_deref(), Some("GET"));
    assert_eq!(names(&tree[0].children), ["a/inner.http.yaml"]);
}

#[test]
fn create_names_collide_case_insensitively() {
    let (_dir, collection) = new_collection();

    let first = collection.create_request("", "Users").unwrap();
    let second = collection.create_request("", "USERS").unwrap();
    let folder = collection.create_folder("", "users").unwrap();

    assert_eq!(first, "users.http.yaml");
    assert_eq!(second, "users-2.http.yaml");
    assert_eq!(folder, "users-3");
}

#[test]
fn reorder_writes_full_list_of_children() {
    let (_dir, collection) = new_collection();
    for name in ["A", "B", "C"] {
        collection.create_request("", name).unwrap();
    }

    let moved = collection.move_node("c.http.yaml", "", 0).unwrap();

    assert_eq!(moved, "c.http.yaml");
    assert_eq!(
        root_order(&collection).unwrap(),
        "order:\n  - c.http.yaml\n  - a.http.yaml\n  - b.http.yaml\n"
    );
    assert_eq!(
        names(&collection.tree()),
        ["c.http.yaml", "a.http.yaml", "b.http.yaml"]
    );
}

#[test]
fn index_counts_positions_after_the_node_is_removed() {
    let (_dir, collection) = new_collection();
    for name in ["A", "B", "C"] {
        collection.create_request("", name).unwrap();
    }
    let target = collection.create_folder("", "T").unwrap();
    collection.create_request(&target, "X").unwrap();
    let root_names =
        |c: &Collection| -> Vec<String> { c.tree().iter().map(|node| node.path.clone()).collect() };

    // Move Up: b at index 1 -> index 0.
    collection.move_node("b.http.yaml", "", 0).unwrap();
    assert_eq!(
        root_names(&collection),
        ["b.http.yaml", "a.http.yaml", "c.http.yaml", "t"]
    );

    // Move Down: b at index 0 -> index 1.
    collection.move_node("b.http.yaml", "", 1).unwrap();
    assert_eq!(
        root_names(&collection),
        ["a.http.yaml", "b.http.yaml", "c.http.yaml", "t"]
    );

    // Drop onto a folder: index = that folder's current child count (append).
    let count = u32::try_from(collection.tree()[3].children.len()).unwrap();
    let moved = collection.move_node("a.http.yaml", &target, count).unwrap();
    let tree = collection.tree();
    let folder = tree.iter().find(|node| node.path == "t").unwrap();
    assert_eq!(moved, "t/a.http.yaml");
    assert_eq!(names(&folder.children), ["t/x.http.yaml", "t/a.http.yaml"]);
}

#[test]
fn reorder_clamps_index_and_drops_stale_entries() {
    let (_dir, collection) = new_collection();
    collection.create_request("", "A").unwrap();
    collection.create_request("", "B").unwrap();
    write(
        &collection.requests_root().join(FOLDER_FILE),
        "order:\n  - gone.http.yaml\n  - ../x\n  - /abs\n  - b.http.yaml\n",
    );

    collection.move_node("b.http.yaml", "", 99).unwrap();

    assert_eq!(
        root_order(&collection).unwrap(),
        "order:\n  - a.http.yaml\n  - b.http.yaml\n"
    );
}

#[test]
fn rename_replaces_listed_entry_and_keeps_position() {
    let (_dir, collection) = new_collection();
    for name in ["A", "B", "C"] {
        collection.create_request("", name).unwrap();
    }
    collection.move_node("c.http.yaml", "", 0).unwrap();

    let renamed = collection.rename("c.http.yaml", "Zed").unwrap();

    assert_eq!(renamed, "zed.http.yaml");
    assert_eq!(names(&collection.tree())[0], "zed.http.yaml");
    assert_eq!(collection.read_request(&renamed).unwrap().name, "Zed");
    assert!(root_order(&collection).unwrap().contains("zed.http.yaml"));
}

#[test]
fn rename_with_same_slug_only_changes_yaml_and_leaves_unlisted_order_alone() {
    let (_dir, collection) = new_collection();
    collection.create_request("", "Users").unwrap();

    let renamed = collection.rename("users.http.yaml", "  USERS! ").unwrap();

    assert_eq!(renamed, "users.http.yaml");
    assert_eq!(collection.read_request(&renamed).unwrap().name, "USERS!");
    assert_eq!(root_order(&collection), None);
}

#[test]
fn rename_folder_writes_name_into_its_folder_file() {
    let (_dir, collection) = new_collection();
    let folder = collection.create_folder("", "Auth").unwrap();
    collection.create_request(&folder, "Login").unwrap();

    let renamed = collection.rename(&folder, "Sign in").unwrap();

    assert_eq!(renamed, "sign-in");
    let tree = collection.tree();
    assert_eq!(tree[0].name, "Sign in");
    assert_eq!(names(&tree[0].children), ["sign-in/login.http.yaml"]);
}

#[test]
fn rename_and_move_refuse_the_root_and_foreign_files() {
    let (_dir, collection) = new_collection();
    write(&collection.requests_root().join("notes.txt"), "x");

    assert!(collection.rename("", "X").is_err());
    assert!(collection.rename("notes.txt", "X").is_err());
    assert!(collection.delete("notes.txt").is_err());
    assert!(collection.move_node("notes.txt", "", 0).is_err());
    assert_eq!(read(&collection.requests_root().join("notes.txt")), "x");
}

#[test]
fn delete_removes_listed_entry_and_folders_recursively() {
    let (_dir, collection) = new_collection();
    let folder = collection.create_folder("", "F").unwrap();
    collection.create_request(&folder, "Inner").unwrap();
    collection.create_request("", "A").unwrap();
    collection.move_node("f", "", 1).unwrap();

    collection.delete("f").unwrap();

    assert_eq!(names(&collection.tree()), ["a.http.yaml"]);
    assert_eq!(
        root_order(&collection).unwrap(),
        "order:\n  - a.http.yaml\n"
    );
    assert_eq!(
        collection.delete("f").unwrap_err().code,
        ErrorCode::NotFound
    );
}

#[test]
fn delete_last_order_entry_removes_the_folder_file() {
    let (_dir, collection) = new_collection();
    collection.create_request("", "A").unwrap();
    collection.move_node("a.http.yaml", "", 0).unwrap();

    collection.delete("a.http.yaml").unwrap();

    assert_eq!(root_order(&collection), None);
}

#[test]
fn move_across_folders_updates_both_orders() {
    let (_dir, collection) = new_collection();
    let target = collection.create_folder("", "T").unwrap();
    collection.create_request(&target, "X").unwrap();
    collection.create_request("", "A").unwrap();
    collection.create_request("", "B").unwrap();
    collection.move_node("a.http.yaml", "", 5).unwrap();

    let moved = collection.move_node("a.http.yaml", &target, 0).unwrap();

    assert_eq!(moved, "t/a.http.yaml");
    assert!(!root_order(&collection).unwrap().contains("a.http.yaml"));
    let tree = collection.tree();
    let folder = tree.iter().find(|node| node.path == "t").unwrap();
    assert_eq!(names(&folder.children), ["t/a.http.yaml", "t/x.http.yaml"]);
    assert_eq!(
        read(&collection.requests_root().join("t").join(FOLDER_FILE)),
        "name: T\norder:\n  - a.http.yaml\n  - x.http.yaml\n"
    );
}

#[test]
fn move_renames_on_collision_in_target() {
    let (_dir, collection) = new_collection();
    let target = collection.create_folder("", "T").unwrap();
    collection.create_request(&target, "A").unwrap();
    collection.create_request("", "A").unwrap();

    let moved = collection.move_node("a.http.yaml", &target, 0).unwrap();

    assert_eq!(moved, "t/a-2.http.yaml");
}

#[test]
fn folder_cannot_move_into_itself_or_a_descendant() {
    let (_dir, collection) = new_collection();
    let outer = collection.create_folder("", "Outer").unwrap();
    let inner = collection.create_folder(&outer, "Inner").unwrap();

    let into_self = collection.move_node(&outer, &outer, 0).unwrap_err();
    let into_child = collection.move_node(&outer, &inner, 0).unwrap_err();

    assert_eq!(into_self.code, ErrorCode::InvalidInput);
    assert_eq!(into_child.code, ErrorCode::InvalidInput);
}

#[test]
fn folder_can_move_into_a_sibling_with_a_shared_prefix() {
    let (_dir, collection) = new_collection();
    let a = collection.create_folder("", "A").unwrap();
    let ab = collection.create_folder("", "A b").unwrap();

    assert_eq!(collection.move_node(&a, &ab, 0).unwrap(), "a-b/a");
}

#[test]
fn write_request_keeps_name_and_unknown_fields() {
    let (_dir, collection) = new_collection();
    write(
        &collection.requests_root().join("r.http.yaml"),
        "extra: 1\nkind: http\nmethod: GET\nname: R\n",
    );

    collection
        .write_request("r.http.yaml", &RequestContent::new("POST", "https://x"))
        .unwrap();

    assert_eq!(
        read(&collection.requests_root().join("r.http.yaml")),
        "extra: 1\nkind: http\nmethod: POST\nname: R\nurl: https://x\n"
    );
    assert!(collection
        .write_request("r.http.yaml", &RequestContent::new("post", ""))
        .is_err());
}

#[test]
fn read_request_refuses_non_requests_and_missing_files() {
    let (_dir, collection) = new_collection();

    assert_eq!(
        collection.read_request("x.txt").unwrap_err().code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        collection.read_request("x.http.yaml").unwrap_err().code,
        ErrorCode::NotFound
    );
}

#[test]
fn invalid_files_are_listed_as_invalid_nodes() {
    let (_dir, collection) = new_collection();
    let root = collection.requests_root();
    write(&root.join("bad.http.yaml"), "kind: http\n<<<<<<< HEAD\n");
    write(&root.join("f").join(FOLDER_FILE), "order: [\n");
    write(
        &root.join(".hidden.http.yaml"),
        "kind: http\nmethod: GET\nname: H\n",
    );
    write(
        &root.join("nul.http.yaml"),
        "kind: http\nmethod: GET\nname: N\n",
    );

    let tree = collection.tree();

    assert_eq!(names(&tree), ["bad.http.yaml", "f"]);
    assert_eq!(tree[0].kind, NodeKind::Invalid);
    assert_eq!(tree[0].name, "bad.http.yaml");
    assert_eq!(names(&tree[1].children), ["f/_folder.yaml"]);
    assert_eq!(tree[1].children[0].kind, NodeKind::Invalid);
    assert!(collection.rename("bad.http.yaml", "Good").is_err());
    collection.delete("f/_folder.yaml").unwrap();
    collection.delete("bad.http.yaml").unwrap();
    assert_eq!(names(&collection.tree()), ["f"]);
}

#[test]
fn nesting_is_limited() {
    let (_dir, collection) = new_collection();
    let deepest = (0..MAX_DEPTH - 1).fold(String::new(), |parent, _| {
        collection.create_folder(&parent, "d").unwrap()
    });

    let error = collection.create_folder(&deepest, "d").unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
}

#[test]
fn moving_a_folder_cannot_exceed_the_nesting_limit() {
    let (_dir, collection) = new_collection();
    let deepest = (0..MAX_DEPTH - 1).fold(String::new(), |parent, _| {
        collection.create_folder(&parent, "d").unwrap()
    });
    let (one_below, _) = paths::split_rel(&deepest);
    let leaf = collection.create_folder("", "Leaf").unwrap();
    let pair = collection.create_folder("", "Pair").unwrap();
    collection.create_folder(&pair, "Child").unwrap();

    let leaf_too_deep = collection.move_node(&leaf, &deepest, 0).unwrap_err();
    let pair_too_deep = collection.move_node(&pair, one_below, 0).unwrap_err();
    let leaf_fits = collection.move_node(&leaf, one_below, 0).unwrap();

    assert_eq!(leaf_too_deep.code, ErrorCode::InvalidInput);
    assert_eq!(pair_too_deep.code, ErrorCode::InvalidInput);
    assert_eq!(leaf_fits, format!("{one_below}/leaf"));
}

#[test]
fn fingerprint_changes_with_content_only() {
    let (_dir, collection) = new_collection();
    let before = collection.fingerprint();
    write(&collection.requests_root().join("notes.txt"), "ignored");
    let unchanged = collection.fingerprint();

    collection.create_request("", "A").unwrap();

    assert_eq!(before, unchanged);
    assert_ne!(before, collection.fingerprint());
}

#[test]
fn arrange_puts_listed_first_then_bytewise() {
    let names = vec![
        "b".to_string(),
        "a".to_string(),
        "C".to_string(),
        "d".to_string(),
    ];
    let order = vec!["d".to_string(), "x".to_string(), "d".to_string()];

    assert_eq!(arrange(&names, &order), ["d", "C", "a", "b"]);
}
