//! Path safety through the public `Store` API (plan §4, §9): hostile relative paths,
//! absolute paths and symlinks never reach outside the collection.

use std::fs;
use std::path::{Path, PathBuf};

use papan_lib::error::{AppError, ErrorCode};
use papan_lib::store::types::{RequestContent, RequestLocation, TreeNode};
use papan_lib::store::Store;

struct World {
    temp: tempfile::TempDir,
    store: Store,
    id: String,
}

impl World {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let outside = temp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(
            outside.join("victim.http.yaml"),
            "kind: http\nmethod: GET\nname: Victim\n",
        )
        .unwrap();
        let mut store = Store::new(&temp.path().join("app")).unwrap();
        let id = store
            .create_collection(temp.path(), "C")
            .unwrap()
            .collection
            .id;
        store.create_folder(&id, "", "Folder").unwrap();
        store.create_request(&id, "", "Inside").unwrap();
        Self { temp, store, id }
    }

    fn requests(&self) -> PathBuf {
        self.temp.path().join("c/requests")
    }

    fn outside(&self) -> PathBuf {
        self.temp.path().join("outside")
    }

    /// Everything under the temp parent except app data, with file contents.
    fn snapshot(&self) -> Vec<(PathBuf, Vec<u8>)> {
        fn walk(dir: &Path, found: &mut Vec<(PathBuf, Vec<u8>)>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let meta = fs::symlink_metadata(&path).unwrap();
                if meta.is_dir() {
                    found.push((path.clone(), Vec::new()));
                    walk(&path, found);
                } else if meta.is_file() {
                    found.push((path.clone(), fs::read(&path).unwrap()));
                } else {
                    found.push((path.clone(), b"<link>".to_vec()));
                }
            }
        }
        let mut found = Vec::new();
        for entry in fs::read_dir(self.temp.path()).unwrap() {
            let path = entry.unwrap().path();
            if path.file_name().unwrap() != "app" {
                walk(&path, &mut found);
            }
        }
        found.sort();
        found
    }

    fn location(&self, path: &str) -> RequestLocation {
        RequestLocation::Collection {
            collection_id: self.id.clone(),
            path: path.to_string(),
        }
    }

    /// Calls every path-taking `Store` method with `path` in each path position.
    fn attempts(&mut self, path: &str) -> Vec<(String, Result<(), AppError>)> {
        let id = self.id.clone();
        let content = RequestContent {
            method: "GET".into(),
            url: "x".into(),
        };
        let scratch = self.store.create_scratch().unwrap().id;
        let location = self.location(path);
        let store = &mut self.store;
        let results = vec![
            (
                "create_folder",
                store.create_folder(&id, path, "N").map(drop),
            ),
            (
                "create_request",
                store.create_request(&id, path, "N").map(drop),
            ),
            ("rename_node", store.rename_node(&id, path, "N").map(drop)),
            ("delete_node", store.delete_node(&id, path).map(drop)),
            (
                "move_node source",
                store.move_node(&id, path, "", 0).map(drop),
            ),
            (
                "move_node target",
                store.move_node(&id, "inside.http.yaml", path, 0).map(drop),
            ),
            ("read_request", store.read_request(&location).map(drop)),
            ("write_request", store.write_request(&location, &content)),
            (
                "move_scratch_to_collection",
                store
                    .move_scratch_to_collection(&scratch, &id, path)
                    .map(drop),
            ),
        ];
        store.delete_scratch(&scratch).unwrap();
        results
            .into_iter()
            .map(|(name, result)| (name.to_string(), result))
            .collect()
    }
}

fn flatten(nodes: &[TreeNode]) -> Vec<String> {
    nodes
        .iter()
        .flat_map(|node| std::iter::once(node.path.clone()).chain(flatten(&node.children)))
        .collect()
}

fn assert_all_rejected(results: Vec<(String, Result<(), AppError>)>, path: &str) {
    for (name, result) in results {
        let error = result.expect_err(&format!("{name}({path:?}) succeeded"));
        assert_eq!(
            error.code,
            ErrorCode::InvalidInput,
            "{name}({path:?}): {error}"
        );
    }
}

#[test]
fn relative_escapes_and_absolute_paths_are_rejected_everywhere() {
    let mut world = World::new();
    let before = world.snapshot();
    let absolute = world.outside().display().to_string();
    let absolute_file = world
        .outside()
        .join("victim.http.yaml")
        .display()
        .to_string();

    for path in [
        "../outside",
        "../outside/victim.http.yaml",
        absolute.as_str(),
        absolute_file.as_str(),
        "folder/../../outside",
    ] {
        let results = world.attempts(path);
        assert_all_rejected(results, path);
    }

    assert_eq!(world.snapshot(), before);
}

#[test]
fn hostile_order_entries_are_ignored() {
    let world = World::new();
    fs::write(
        world.requests().join("_folder.yaml"),
        "order:\n  - ../outside\n  - /etc\n  - inside.http.yaml\n",
    )
    .unwrap();

    let tree = world.store.read_tree(&world.id).unwrap();

    assert_eq!(flatten(&tree.children), ["inside.http.yaml", "folder"]);
}

#[cfg(unix)]
mod symlinks {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn symlinked_file_and_directory_are_not_listed_or_usable() {
        let mut world = World::new();
        symlink(
            world.outside().join("victim.http.yaml"),
            world.requests().join("link.http.yaml"),
        )
        .unwrap();
        symlink(world.outside(), world.requests().join("linkdir")).unwrap();
        let before = world.snapshot();

        let tree = world.store.read_tree(&world.id).unwrap();
        for path in ["link.http.yaml", "linkdir", "linkdir/victim.http.yaml"] {
            let results = world.attempts(path);
            assert_all_rejected(results, path);
        }

        assert_eq!(flatten(&tree.children), ["folder", "inside.http.yaml"]);
        assert_eq!(world.snapshot(), before);
    }

    #[test]
    fn a_collection_folder_replaced_by_a_symlink_is_unavailable() {
        let mut world = World::new();
        let root = world.temp.path().join("c");
        let elsewhere = world.temp.path().join("elsewhere");
        fs::rename(&root, &elsewhere).unwrap();
        symlink(&elsewhere, &root).unwrap();

        let tree = world.store.read_tree(&world.id).unwrap();
        let created = world.store.create_request(&world.id, "", "R");

        assert!(!tree.collection.available);
        assert!(world.store.watch_targets().is_empty());
        assert_eq!(created.unwrap_err().code, ErrorCode::InvalidInput);
        assert!(!elsewhere.join("requests/r.http.yaml").exists());
    }

    #[test]
    fn symlinked_requests_folder_refuses_to_open() {
        let temp = tempfile::tempdir().unwrap();
        let outside = temp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        let root = temp.path().join("c");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("papan.collection.yaml"), "name: C\nversion: 1\n").unwrap();
        symlink(&outside, root.join("requests")).unwrap();
        let mut store = Store::new(&temp.path().join("app")).unwrap();

        let error = store.open_collection(&root).unwrap_err();

        assert_eq!(error.code, ErrorCode::InvalidInput);
        assert!(fs::read_dir(&outside).unwrap().next().is_none());
    }
}
