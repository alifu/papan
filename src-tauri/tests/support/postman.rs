//! Shared helpers for the Postman integration tests (plan 09 §10): a store with a memory
//! vault, fixture paths, and on-disk snapshots to search for leaked secrets.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use papan_lib::error::AppError;
use papan_lib::interop::postman::export_flow::{build_export, write_bundle};
use papan_lib::interop::postman::flow::{commit_into, commit_new, plan_paths};
use papan_lib::interop::postman::types::{
    ExportOptions, GlobalsClash, ImportOptions, ImportTarget, PostmanExportResult,
    PostmanImportResult,
};
use papan_lib::store::types::{CollectionTree, NodeKind, TreeNode};
use papan_lib::store::Store;
use papan_lib::vars::types::SecretStorageMode;
use papan_lib::vars::vault::MemoryVault;
use papan_lib::vars::VarsService;

pub struct World {
    pub app: tempfile::TempDir,
    pub work: tempfile::TempDir,
    pub store: Store,
    pub vars: VarsService,
    pub vault: Arc<MemoryVault>,
}

impl World {
    pub fn new() -> Self {
        let app = tempfile::tempdir().unwrap();
        let vault = Arc::new(MemoryVault::new(SecretStorageMode::SessionOnly));
        Self {
            store: Store::new(app.path()).unwrap(),
            vars: VarsService::new(vault.clone()),
            work: tempfile::tempdir().unwrap(),
            app,
            vault,
        }
    }

    /// Imports `files` (paths under `tests/fixtures/postman/`) as a new collection.
    pub fn import_new(
        &mut self,
        files: &[&str],
        options: &ImportOptions,
    ) -> Result<PostmanImportResult, AppError> {
        let paths: Vec<PathBuf> = files.iter().map(|f| fixture(f)).collect();
        self.import_paths(&paths, options)
    }

    pub fn import_paths(
        &mut self,
        paths: &[PathBuf],
        options: &ImportOptions,
    ) -> Result<PostmanImportResult, AppError> {
        let (plan, _) = plan_paths(paths)?;
        match options.target {
            ImportTarget::NewCollection => commit_new(
                &plan,
                options,
                self.work.path(),
                &mut self.store,
                &self.vars,
            ),
            ImportTarget::Collection { .. } => {
                commit_into(&plan, options, &mut self.store, &self.vars)
            }
        }
    }

    pub fn tree(&self, id: &str) -> CollectionTree {
        self.store.read_tree(id).unwrap()
    }

    pub fn root(&self, id: &str) -> PathBuf {
        PathBuf::from(self.tree(id).collection.path)
    }

    /// Exports into a fresh folder; returns it with the result.
    pub fn export(
        &self,
        options: &ExportOptions,
    ) -> Result<(tempfile::TempDir, PostmanExportResult), AppError> {
        let out = tempfile::tempdir().unwrap();
        let bundle = build_export(&self.store, self.vault.as_ref(), options)?;
        let result = write_bundle(out.path(), bundle)?;
        Ok((out, result))
    }
}

pub fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/postman")
        .join(rel)
}

pub fn import_options(store_secret_values: bool) -> ImportOptions {
    ImportOptions {
        target: ImportTarget::NewCollection,
        secret_names: Vec::new(),
        store_secret_values,
        import_scripts: true,
        globals_clash: GlobalsClash::Keep,
    }
}

pub fn export_options(id: &str) -> ExportOptions {
    ExportOptions {
        collection_id: id.to_string(),
        environments: Vec::new(),
        include_globals: false,
        include_secret_values: false,
    }
}

/// Every regular file under `dir` by relative path (`/`-separated).
pub fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    collect(dir, dir, &mut out);
    out
}

fn collect(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(root, &path, out);
        } else {
            let rel = path.strip_prefix(root).unwrap();
            let key = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            out.insert(key, std::fs::read(&path).unwrap());
        }
    }
}

pub fn any_contains(files: &BTreeMap<String, Vec<u8>>, needle: &str) -> bool {
    files
        .values()
        .any(|bytes| bytes.windows(needle.len()).any(|w| w == needle.as_bytes()))
}

pub fn requests(nodes: &[TreeNode]) -> Vec<&TreeNode> {
    let mut found = Vec::new();
    for node in nodes {
        match node.kind {
            NodeKind::Request => found.push(node),
            _ => found.extend(requests(&node.children)),
        }
    }
    found
}

pub fn folder_count(nodes: &[TreeNode]) -> usize {
    nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Folder)
        .map(|n| 1 + folder_count(&n.children))
        .sum()
}
