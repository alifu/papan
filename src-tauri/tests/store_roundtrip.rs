//! Golden tests for the on-disk format (plan §9) against `tests/fixtures/`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use papan_lib::error::ErrorCode;
use papan_lib::store::collection::Collection;
use papan_lib::store::env_model::{EnvFile, GlobalsFile};
use papan_lib::store::model::{CollectionFile, RequestFile};
use papan_lib::store::types::{Body, NodeKind, RequestContent, RequestKind, SoapVersion, TreeNode};

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

/// Environment fixtures are named `env-*`; everything else is a request file.
fn is_env_fixture(label: &str) -> bool {
    label.starts_with("env-")
}

fn canonical(bytes: &[u8], label: &str) -> String {
    if is_env_fixture(label) {
        return canonical_env(bytes, label);
    }
    RequestFile::parse(label, bytes).unwrap().to_yaml().unwrap()
}

fn canonical_env(bytes: &[u8], label: &str) -> String {
    EnvFile::parse(label, bytes).unwrap().to_yaml().unwrap()
}

fn assert_fixed_point(path: &Path, emit: impl Fn(&[u8], &str) -> String) {
    let bytes = fs::read(path).unwrap();
    let name = file_name(path);
    let written = emit(&bytes, &name);
    assert_eq!(written, String::from_utf8(bytes).unwrap(), "{name}");
}

#[test]
fn canonical_variable_files_are_fixed_points() {
    let envs = files_in(&fixtures().join("canonical/environments"), ".env.yaml");
    assert!(envs.len() >= 8, "fixtures missing");
    for path in &envs {
        assert_fixed_point(path, canonical_env);
    }
    assert_fixed_point(
        &fixtures().join("canonical/globals/globals.yaml"),
        |bytes, label| GlobalsFile::parse(label, bytes).unwrap().to_yaml().unwrap(),
    );
    assert_fixed_point(
        &fixtures().join("canonical/collection-vars/papan.collection.yaml"),
        |bytes, label| {
            CollectionFile::parse(label, bytes)
                .unwrap()
                .to_yaml()
                .unwrap()
        },
    );
}

#[test]
fn editing_one_variable_keeps_every_other_line() {
    let path = fixtures().join("canonical/environments/future-fields.env.yaml");
    let original = fs::read_to_string(&path).unwrap();
    let file = EnvFile::parse("future-fields.env.yaml", original.as_bytes()).unwrap();
    let mut variables = file.variables.clone();
    variables.get_mut("region").unwrap().value = "us-east-1".into();

    let written = EnvFile { variables, ..file }.to_yaml().unwrap();

    assert_eq!(written, original.replace("eu-west-1", "us-east-1"));
}

#[test]
fn invalid_env_files_name_the_file_and_the_field() {
    let dir = fixtures().join("invalid");
    let files = files_in(&dir, ".env.yaml");
    assert!(files.len() >= 8, "fixtures missing");

    for path in files {
        let name = file_name(&path);
        let field = fs::read_to_string(dir.join(name.replace(".env.yaml", ".field.txt"))).unwrap();

        let error = EnvFile::parse(&name, &fs::read(&path).unwrap()).unwrap_err();

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
    assert!(files.len() >= 18, "fixtures missing");

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
            .write_request(&path, &file.content().unwrap())
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
            &RequestContent::new("PATCH", "https://api.example.com/items"),
        )
        .unwrap();

    let expected = fs::read_to_string(&source)
        .unwrap()
        .replace("method: POST", "method: PATCH");
    assert_eq!(fs::read_to_string(&target).unwrap(), expected);
}

#[test]
fn rewriting_with_an_edited_url_keeps_unknown_and_unsupported_sections() {
    for fixture in ["unsupported-auth", "unknown-nested"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("c");
        fs::create_dir_all(root.join("requests")).unwrap();
        fs::write(root.join("papan.collection.yaml"), "name: C\nversion: 1\n").unwrap();
        let name = format!("{fixture}.http.yaml");
        let source = fixtures().join("canonical/requests").join(&name);
        let target = root.join("requests").join(&name);
        fs::copy(&source, &target).unwrap();
        let collection = Collection::open(&root).unwrap();
        let content = collection.read_request(&name).unwrap().content().unwrap();
        let edited = RequestContent {
            url: "https://api.example.com/edited".into(),
            ..content.clone()
        };

        collection.write_request(&name, &edited).unwrap();

        let expected = fs::read_to_string(&source).unwrap().replace(
            &format!("url: {}", content.url),
            "url: https://api.example.com/edited",
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), expected, "{fixture}");
    }
}

#[test]
fn m6_graphql_and_soap_requests_are_fixed_points() {
    let files = files_in(&fixtures().join("canonical/m6"), ".http.yaml");
    assert!(files.len() >= 6, "fixtures missing");

    for path in &files {
        assert_fixed_point(path, canonical);
    }
}

#[test]
fn m6_requests_parse_to_their_kind_and_round_trip_through_content() {
    let read = |name: &str| {
        let bytes = fs::read(fixtures().join("canonical/m6").join(name)).unwrap();
        RequestFile::parse(name, &bytes).unwrap()
    };
    let gql = read("graphql-get-user.http.yaml");
    let soap = read("soap-add.http.yaml");

    let gql_content = gql.content().unwrap();
    let soap_content = soap.content().unwrap();

    assert_eq!(gql_content.kind, RequestKind::Graphql);
    assert_eq!(
        gql_content.body,
        Body::Graphql {
            query: "query GetUser($id: ID!) {\n  user(id: $id) { id name }\n}\n".into(),
            variables: "{ \"id\": \"{{userId}}\" }\n".into(),
            operation_name: "GetUser".into(),
        }
    );
    assert_eq!(soap_content.kind, RequestKind::Soap);
    assert_eq!(soap_content.soap.version, SoapVersion::V12);
    assert_eq!(soap_content.soap.action, "http://tempuri.org/Add");
    for (file, content) in [(&gql, &gql_content), (&soap, &soap_content)] {
        let written = file.with_content(content).unwrap();
        assert_eq!(written.to_yaml().unwrap(), file.to_yaml().unwrap());
    }
}

#[test]
fn editing_a_soap_request_keeps_unknown_soap_keys() {
    let name = "soap-file-wsdl.http.yaml";
    let source = fixtures().join("canonical/m6").join(name);
    let file = RequestFile::parse(name, &fs::read(&source).unwrap()).unwrap();
    let content = file.content().unwrap();
    let edited = RequestContent {
        url: "http://example.test/edited".into(),
        ..content.clone()
    };

    let written = file.with_content(&edited).unwrap().to_yaml().unwrap();

    let expected = fs::read_to_string(&source)
        .unwrap()
        .replace(&content.url, "http://example.test/edited");
    assert_eq!(written, expected);
}

#[test]
fn collection_tree_carries_the_request_kind() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("c");
    copy_dir(&fixtures().join("canonical/m6"), &root.join("requests"));
    fs::write(root.join("papan.collection.yaml"), "name: C\nversion: 1\n").unwrap();

    let tree = Collection::open(&root).unwrap().tree();
    let kind_of = |path: &str| tree.iter().find(|n| n.path == path).unwrap().request_kind;
    assert_eq!(
        kind_of("graphql-minimal.http.yaml"),
        Some(RequestKind::Graphql)
    );
    assert_eq!(kind_of("soap-add.http.yaml"), Some(RequestKind::Soap));
    assert!(tree.iter().all(|node| node.kind == NodeKind::Request));
}
