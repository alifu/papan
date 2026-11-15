//! Hostile inputs (plan 09 §4, §10): every case ends in a clear error or a safe import, never
//! a panic, never a file outside the new collection, never a partial result.

#[path = "support/postman.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use papan_lib::error::AppError;
use papan_lib::interop::postman::flow::plan_paths;
use serde_json::json;
use support::{fixture, import_options, snapshot, World};

const SCHEMA: &str = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";
const TIME_BUDGET: Duration = Duration::from_secs(20);

fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn collection(items: serde_json::Value) -> Vec<u8> {
    let doc = json!({ "info": { "name": "Hostile", "schema": SCHEMA }, "item": items });
    serde_json::to_vec(&doc).unwrap()
}

fn request(name: &str) -> serde_json::Value {
    json!({ "name": name, "request": { "method": "GET", "url": "https://example.test" } })
}

fn rejected(bytes: &[u8]) -> AppError {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "evil.postman_collection.json", bytes);
    let error = plan_paths(&[path]).expect_err("the file must be refused");
    let shown = format!("{error:?}");
    let leaked = dir.path().to_string_lossy().into_owned();
    assert!(
        !shown.contains(&leaked),
        "the error names a full path: {shown}"
    );
    error
}

fn nested(depth: usize) -> Vec<u8> {
    let mut text = String::from("{\"a\":");
    text.push_str(&"[".repeat(depth));
    text.push_str(&"]".repeat(depth));
    text.push('}');
    text.into_bytes()
}

#[test]
fn a_six_mebibyte_file_is_refused_by_name() {
    let filler = "x".repeat(6 * 1024 * 1024);

    let error = rejected(filler.as_bytes());

    assert!(error.message.contains("evil.postman_collection.json"));
    assert!(error.message.contains("5 MiB"));
}

#[test]
fn deep_nesting_is_refused_before_parsing() {
    let error = rejected(&nested(200));

    assert!(error.message.contains("64"), "{}", error.message);
}

#[test]
fn twenty_thousand_items_are_refused_in_bounded_time() {
    let items: Vec<_> = (0..20_000).map(|n| request(&format!("r{n}"))).collect();
    let bytes = collection(json!(items));
    assert!(bytes.len() < 5 * 1024 * 1024);
    let started = Instant::now();

    let error = rejected(&bytes);

    assert!(started.elapsed() < TIME_BUDGET);
    assert!(error.message.contains("evil.postman_collection.json"));
}

#[test]
fn not_json_array_roots_v1_and_binary_files_are_refused_with_a_reason() {
    let cases: [(&[u8], &str); 6] = [
        (b"this is not json", "JSON"),
        (b"[1, 2, 3]", "Postman"),
        (b"\xff\xfe\x00\x01 not utf-8", "UTF-8"),
        (br#"{"id":"x","name":"Old","requests":[{"id":"r","url":"http://a"}]}"#, "v1"),
        (br#"{"collections":[],"environments":[]}"#, "Data dumps"),
        (br#"{"info":{"name":"x","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},"item":{}}"#, "item"),
    ];
    for (bytes, expected) in cases {
        let error = rejected(bytes);

        assert!(
            error.message.contains(expected),
            "{:?} gave {}",
            String::from_utf8_lossy(bytes),
            error.message
        );
    }
}

#[test]
fn a_utf8_byte_order_mark_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = b"\xEF\xBB\xBF".to_vec();
    bytes.extend(collection(json!([request("One")])));
    let path = write(dir.path(), "bom.postman_collection.json", &bytes);

    let (plan, _) = plan_paths(&[path]).unwrap();

    assert_eq!(plan.summaries[0].requests, 1);
}

#[test]
fn more_than_twenty_files_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "a.json", &collection(json!([request("One")])));
    let paths = vec![path; 21];

    let error = plan_paths(&paths).unwrap_err();

    assert!(error.message.contains("20"));
}

#[cfg(unix)]
#[test]
fn a_symlink_as_input_path_is_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let link = dir.path().join("link.postman_collection.json");
    std::os::unix::fs::symlink(fixture("v21/basic.postman_collection.json"), &link).unwrap();

    let error = plan_paths(&[link]).unwrap_err();

    assert!(!format!("{error:?}").contains(&dir.path().to_string_lossy().into_owned()));
}

#[test]
fn hostile_names_never_leave_the_new_collection() {
    let names = [
        "../../etc",
        "..",
        "a/b",
        "a\\b",
        "CON",
        "nul",
        "x\u{0}y",
        "\u{7}bell",
        ".",
        "  ",
        &"n".repeat(10_240),
    ];
    let items: Vec<_> = names.iter().map(|n| request(n)).collect();
    let folders: Vec<_> = names
        .iter()
        .map(|n| json!({ "name": n, "item": [request("inner")] }))
        .collect();
    let bytes = collection(json!([items, folders].concat()));
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "names.postman_collection.json", &bytes);
    let mut world = World::new();

    let result = world.import_paths(&[path], &import_options(false)).unwrap();

    let root = world.root(&result.collection_id.unwrap());
    let entries: Vec<_> = std::fs::read_dir(world.work.path()).unwrap().collect();
    assert_eq!(entries.len(), 1, "only the collection folder may appear");
    for key in snapshot(&root).keys() {
        for segment in key.split('/') {
            assert!(
                segment != ".." && segment != "." && !segment.is_empty(),
                "{key}"
            );
            assert!(
                !segment.chars().any(char::is_control) && !segment.contains('\\'),
                "{key}"
            );
            let stem = segment.split('.').next().unwrap_or("").to_ascii_lowercase();
            assert!(
                !["con", "nul", "prn", "aux"].contains(&stem.as_str()),
                "{key}"
            );
            assert!(segment.len() < 200, "{key}");
        }
    }
}

#[test]
fn three_hundred_equal_names_get_distinct_files() {
    let items: Vec<_> = (0..300).map(|_| request("Same")).collect();
    let dir = tempfile::tempdir().unwrap();
    let path = write(
        dir.path(),
        "same.postman_collection.json",
        &collection(json!(items)),
    );
    let mut world = World::new();

    let result = world.import_paths(&[path], &import_options(false)).unwrap();

    let tree = world.tree(&result.collection_id.unwrap());
    assert_eq!(support::requests(&tree.children).len(), 300);
}

#[test]
fn every_hostile_fixture_ends_in_an_error_or_a_contained_import() {
    let dir = fixture("hostile");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let started = Instant::now();
        let mut world = World::new();

        let outcome = world.import_paths(std::slice::from_ref(&path), &import_options(false));

        assert!(started.elapsed() < TIME_BUDGET, "{path:?}");
        let entries = std::fs::read_dir(world.work.path()).unwrap().count();
        match outcome {
            Ok(_) => assert_eq!(entries, 1, "{path:?}"),
            Err(error) => {
                assert_eq!(
                    entries, 0,
                    "{path:?}: partial result after {}",
                    error.message
                );
                assert!(!format!("{error:?}").contains(path.parent().unwrap().to_str().unwrap()));
            }
        }
    }
}

#[test]
fn one_bad_file_among_good_ones_imports_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let bad = write(dir.path(), "bad.postman_collection.json", b"{ nope");
    let mut world = World::new();

    let error = world
        .import_paths(
            &[fixture("v21/basic.postman_collection.json"), bad],
            &import_options(false),
        )
        .unwrap_err();

    assert!(error.message.contains("bad.postman_collection.json"));
    assert_eq!(std::fs::read_dir(world.work.path()).unwrap().count(), 0);
}
