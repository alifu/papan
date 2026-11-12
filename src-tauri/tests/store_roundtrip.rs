//! Golden tests for the on-disk format (plan §9) against `tests/fixtures/`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use papan_lib::error::ErrorCode;
use papan_lib::store::collection::Collection;
use papan_lib::store::model::RequestFile;
use papan_lib::store::types::{NodeKind, TreeNode};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn files_in(dir: &Path, suffix: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.to_string_lossy().ends_with(suffix))
        .collect();
    files.sort();
    files
}

fn file_name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

fn canonical(bytes: &[u8], label: &str) -> String {
    RequestFile::parse(label, bytes).unwrap().to_yaml().unwrap()
}

#[test]
fn canonical_requests_are_fixed_points() {
    let files = files_in(&fixtures().join("canonical/requests"), ".http.yaml");
    assert!(files.len() >= 10, "fixtures missing");

    let mismatches: Vec<String> = files
        .iter()
        .filter_map(|path| {
            let bytes = fs::read(path).unwrap();
            let written = canonical(&bytes, &file_name(path));
            (written.as_bytes() != bytes.as_slice()).then(|| {
                format!(
                    "{}:\n--- fixture\n{}--- emitted\n{written}",
                    file_name(path),
                    String::from_utf8_lossy(&bytes)
                )
            })
        })
        .collect();

    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
fn normalise_inputs_become_their_expected_form() {
    let dir = fixtures().join("normalise");
    let inputs = files_in(&dir, ".in.yaml");
    assert!(!inputs.is_empty(), "fixtures missing");

    for input in inputs {
        let name = file_name(&input);
        let expected_path = dir.join(name.replace(".in.yaml", ".expected.yaml"));
        let expected = fs::read_to_string(&expected_path).unwrap();

        let once = canonical(&fs::read(&input).unwrap(), &name);
        let twice = canonical(once.as_bytes(), &name);

        assert_eq!(once, expected, "{name}");
        assert_eq!(twice, once, "{name} is not a fixed point");
    }
}

#[test]
fn invalid_files_name_the_file_and_the_field() {
    let dir = fixtures().join("invalid");
    let files = files_in(&dir, ".http.yaml");
    assert_eq!(files.len(), 9, "fixtures missing");

    for path in files {
        let name = file_name(&path);
        let field_file = dir.join(name.replace(".http.yaml", ".field.txt"));
        let field = fs::read_to_string(&field_file).unwrap();
        let started = Instant::now();

        let error = RequestFile::parse(&name, &fs::read(&path).unwrap()).unwrap_err();

        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{name} was slow"
        );
        assert_eq!(error.code, ErrorCode::InvalidInput, "{name}");
        assert!(error.message.contains(&name), "{name}: {}", error.message);
        assert!(
            error.message.contains(field.trim()),
            "{name}: `{}` does not name `{}`",
            error.message,
            field.trim()
        );
    }
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Every file under `dir` with its bytes and modification time, sorted by path.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>, SystemTime)> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(&current).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let modified = fs::metadata(&path).unwrap().modified().unwrap();
                found.push((path.clone(), fs::read(&path).unwrap(), modified));
            }
        }
    }
    found.sort();
    found
}

fn outline(nodes: &[TreeNode]) -> Vec<String> {
    nodes
        .iter()
        .flat_map(|node| {
            let kind = match node.kind {
                NodeKind::Folder => "folder",
                NodeKind::Request => "request",
                NodeKind::Invalid => "invalid",
            };
            std::iter::once(format!("{} {kind} {}", node.path, node.name))
                .chain(outline(&node.children))
        })
        .collect()
}

fn request_paths(nodes: &[TreeNode]) -> Vec<String> {
    nodes
        .iter()
        .flat_map(|node| match node.kind {
            NodeKind::Request => vec![node.path.clone()],
            _ => request_paths(&node.children),
        })
        .collect()
}

#[test]
fn collection_basic_opens_in_order_and_rewrites_byte_identically() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("collection-basic");
    copy_dir(&fixtures().join("canonical/collection-basic"), &root);
    let before = snapshot(&root);

    let collection = Collection::open(&root).unwrap();
    let tree = collection.tree();
    for path in request_paths(&tree) {
        let file = collection.read_request(&path).unwrap();
        collection
            .write_request(&path, &file.method, &file.url)
            .unwrap();
    }

    assert_eq!(collection.name, "Collection Basic");
    assert_eq!(
        outline(&tree),
        [
            "ping.http.yaml request Ping",
            "auth folder Auth",
            "auth/login.http.yaml request Login",
            "auth/refresh.http.yaml request Refresh",
            "health.http.yaml request Health",
            "users folder users",
            "users/list.http.yaml request List users",
        ]
    );
    assert_eq!(snapshot(&root), before);
}

#[test]
fn writing_keeps_unknown_fields_of_a_newer_papan() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("c");
    fs::create_dir_all(root.join("requests")).unwrap();
    fs::write(root.join("papan.collection.yaml"), "name: C\nversion: 1\n").unwrap();
    let source = fixtures().join("canonical/requests/future-fields.http.yaml");
    let target = root.join("requests/future-fields.http.yaml");
    fs::copy(&source, &target).unwrap();
    let collection = Collection::open(&root).unwrap();

    collection
        .write_request(
            "future-fields.http.yaml",
            "PATCH",
            "https://api.example.com/items",
        )
        .unwrap();

    let expected = fs::read_to_string(&source)
        .unwrap()
        .replace("method: POST", "method: PATCH");
    assert_eq!(fs::read_to_string(&target).unwrap(), expected);
}
