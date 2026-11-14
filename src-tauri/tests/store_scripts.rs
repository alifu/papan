//! Plan 05 §3 and §5: request, folder and collection scripts on disk.

use std::fs;
use std::path::PathBuf;

use papan_lib::error::ErrorCode;
use papan_lib::store::types::{RequestContent, RequestLocation, ScriptTarget, Scripts};
use papan_lib::store::Store;

const PRE: &str = "pm.environment.set(\"ts\", Date.now());\n";
const POST: &str = "const body = pm.response.json();\npm.environment.set(\"token\", body.token);\n";

struct Fixture {
    _app: tempfile::TempDir,
    _work: tempfile::TempDir,
    store: Store,
    id: String,
    root: PathBuf,
}

fn fixture() -> Fixture {
    let app = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let mut store = Store::new(app.path()).unwrap();
    let tree = store.create_collection(work.path(), "C").unwrap();
    store
        .create_folder(&tree.collection.id, "", "Auth")
        .unwrap();
    Fixture {
        _app: app,
        _work: work,
        store,
        id: tree.collection.id,
        root: PathBuf::from(tree.collection.path),
    }
}

fn scripts() -> Scripts {
    Scripts {
        pre_request: PRE.into(),
        post_response: POST.into(),
    }
}

fn folder(f: &Fixture) -> ScriptTarget {
    ScriptTarget::Folder {
        collection_id: f.id.clone(),
        path: "auth".into(),
    }
}

fn collection(f: &Fixture) -> ScriptTarget {
    ScriptTarget::Collection {
        collection_id: f.id.clone(),
    }
}

#[test]
fn request_scripts_are_written_as_block_scalars_and_read_back() {
    let mut f = fixture();
    f.store.create_request(&f.id, "", "Login").unwrap();
    let location = RequestLocation::Collection {
        collection_id: f.id.clone(),
        path: "login.http.yaml".into(),
    };
    let content = RequestContent {
        scripts: scripts(),
        ..RequestContent::new("POST", "https://x")
    };

    f.store.write_request(&location, &content).unwrap();
    let text = fs::read_to_string(f.root.join("requests/login.http.yaml")).unwrap();

    assert!(
        text.contains(
            "scripts:\n  post_response: |\n    const body = pm.response.json();\n    \
             pm.environment.set(\"token\", body.token);\n  pre_request: |\n"
        ),
        "{text}"
    );
    assert_eq!(f.store.read_request(&location).unwrap().content, content);
}

#[test]
fn folder_scripts_round_trip_and_clear_keeps_the_name() {
    let mut f = fixture();
    let file = f.root.join("requests/auth/_folder.yaml");

    f.store.write_scripts(&folder(&f), &scripts()).unwrap();
    let written = fs::read_to_string(&file).unwrap();
    f.store
        .write_scripts(&folder(&f), &Scripts::default())
        .unwrap();

    assert!(written.contains("pre_request: |\n"), "{written}");
    assert_eq!(fs::read_to_string(&file).unwrap(), "name: Auth\n");
    assert_eq!(
        f.store.read_scripts(&folder(&f)).unwrap(),
        Scripts::default()
    );
}

#[test]
fn folder_scripts_alone_keep_the_folder_file() {
    let mut f = fixture();
    let file = f.root.join("requests/auth/_folder.yaml");
    fs::write(&file, "future: 1\n").unwrap();

    f.store.write_scripts(&folder(&f), &scripts()).unwrap();

    assert_eq!(f.store.read_scripts(&folder(&f)).unwrap(), scripts());
    assert!(fs::read_to_string(&file)
        .unwrap()
        .starts_with("future: 1\n"));
}

#[test]
fn collection_scripts_round_trip_and_keep_unknown_keys() {
    let mut f = fixture();
    let file = f.root.join("papan.collection.yaml");
    let original = fs::read_to_string(&file).unwrap();
    fs::write(&file, format!("{original}zz_future: keep\n")).unwrap();

    f.store.write_scripts(&collection(&f), &scripts()).unwrap();
    let read = f.store.read_scripts(&collection(&f)).unwrap();

    assert_eq!(read, scripts());
    assert!(fs::read_to_string(&file)
        .unwrap()
        .contains("zz_future: keep\n"));
}

#[test]
fn our_own_script_writes_do_not_look_like_outside_changes() {
    let mut f = fixture();

    f.store.write_scripts(&collection(&f), &scripts()).unwrap();
    f.store.write_scripts(&folder(&f), &scripts()).unwrap();

    assert!(!f.store.refresh_fingerprint(&f.id));
}

#[test]
fn bad_targets_and_oversized_scripts_are_rejected() {
    let mut f = fixture();
    let huge = Scripts {
        pre_request: "x".repeat(256 * 1024 + 1),
        post_response: String::new(),
    };
    let root = ScriptTarget::Folder {
        collection_id: f.id.clone(),
        path: String::new(),
    };
    let missing = ScriptTarget::Folder {
        collection_id: f.id.clone(),
        path: "nope".into(),
    };
    let escape = ScriptTarget::Folder {
        collection_id: f.id.clone(),
        path: "../..".into(),
    };

    let too_big = f.store.write_scripts(&collection(&f), &huge).unwrap_err();

    assert!(
        too_big.message.contains("scripts.pre_request"),
        "{}",
        too_big.message
    );
    assert!(f.store.read_scripts(&root).is_err());
    assert_eq!(
        f.store.read_scripts(&missing).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert!(f.store.write_scripts(&escape, &scripts()).is_err());
}
