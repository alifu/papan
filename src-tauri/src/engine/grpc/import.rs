//! Import of a picked `.proto` file (plan 08 §5): it is compiled with its own folder as
//! the only include root (confined like any other proto read), and the file plus what it
//! imports are copied into the protos root under the same relative names. Files are
//! copied, not referenced, so no grant on the picked path is kept.

use std::path::Path;

use super::protos::{compile, load, ConfinedSource, Loaded, PROTO_SUFFIX};
use super::types::ProtoImport;
use crate::error::AppError;
use crate::store::fsio::{self, EntryKind};
use crate::store::paths;

enum Planned {
    Write,
    Same,
}

fn plan(root: &Path, name: &str, text: &str) -> Result<Planned, AppError> {
    let not_allowed =
        || AppError::invalid_input(format!("`{name}` cannot be copied into the protos folder."));
    let target = paths::resolve(root, name).map_err(|_| not_allowed())?;
    match fsio::entry_kind(&target) {
        None => Ok(Planned::Write),
        Some(EntryKind::File) => {
            let same = std::fs::read(&target).is_ok_and(|bytes| bytes == text.as_bytes());
            if same {
                Ok(Planned::Same)
            } else {
                Err(AppError::invalid_input(format!(
                    "protos/{name} already exists with other content."
                )))
            }
        }
        Some(_) => Err(not_allowed()),
    }
}

fn write(root: &Path, name: &str, text: &str) -> Result<(), AppError> {
    let target = paths::resolve(root, name)?;
    if let Some(parent) = target.parent() {
        fsio::create_dir_all(parent)?;
    }
    // Checked again after the folders exist: a symlink must not have appeared in between.
    let target = paths::resolve(root, name)?;
    fsio::write_atomic(&target, text.as_bytes())?;
    Ok(())
}

fn closure_of(picked: &Path) -> Result<(String, Loaded), AppError> {
    let unusable = || AppError::invalid_input("Pick a .proto file.");
    let file_name = picked
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| name.ends_with(PROTO_SUFFIX))
        .ok_or_else(unusable)?;
    let folder = picked.parent().ok_or_else(unusable)?;
    let folder = fsio::canonicalize(folder).map_err(|_| unusable())?;
    let source = ConfinedSource::new(&folder, &[])?;
    let loaded = load(&source, &[file_name.to_string()])?;
    compile(&loaded).map_err(|error| AppError {
        message: format!(
            "{} Imports are looked up in the folder of the picked file.",
            error.message
        ),
        ..error
    })?;
    Ok((file_name.to_string(), loaded))
}

/// Copies `picked` and its import closure into `protos_root` (created when missing).
/// `added` is the picked file's path in the protos root. Nothing is written when any
/// target exists with other content. Blocking.
pub fn import_proto(picked: &Path, protos_root: &Path) -> Result<ProtoImport, AppError> {
    let (entry, loaded) = closure_of(picked)?;
    let files: Vec<(String, &str)> = loaded
        .names()
        .into_iter()
        .filter_map(|name| loaded.source(&name).map(|text| (name.clone(), text)))
        .collect();
    fsio::create_dir_all(protos_root)?;
    let planned: Vec<Planned> = files
        .iter()
        .map(|(name, text)| plan(protos_root, name, text))
        .collect::<Result<_, _>>()?;
    let mut warnings = Vec::new();
    for ((name, text), planned) in files.iter().zip(&planned) {
        match planned {
            Planned::Write => write(protos_root, name, text)?,
            Planned::Same => warnings.push(format!("protos/{name} was already there.")),
        }
    }
    Ok(ProtoImport {
        added: vec![entry],
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: &str = "syntax = \"proto3\";\nimport \"sub/dep.proto\";\n\
                        import \"google/protobuf/empty.proto\";\nmessage M { Dep dep = 1; }\n";
    const DEP: &str = "syntax = \"proto3\";\nmessage Dep {}\n";

    fn picked_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("main.proto"), MAIN).unwrap();
        std::fs::write(dir.path().join("sub/dep.proto"), DEP).unwrap();
        dir
    }

    fn files_in(root: &Path) -> Vec<String> {
        let mut found = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(dir).unwrap().map(Result::unwrap) {
                if entry.path().is_dir() {
                    pending.push(entry.path());
                } else {
                    let rel = entry.path().strip_prefix(root).unwrap().to_path_buf();
                    found.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        found.sort();
        found
    }

    #[test]
    fn the_import_closure_is_copied_without_well_known_types() {
        let source = picked_dir();
        let target = tempfile::tempdir().unwrap();
        let root = target.path().join("protos");

        let imported = import_proto(&source.path().join("main.proto"), &root).unwrap();

        assert_eq!(imported.added, ["main.proto"]);
        assert!(imported.warnings.is_empty());
        assert_eq!(files_in(&root), ["main.proto", "sub/dep.proto"]);
        assert_eq!(
            std::fs::read_to_string(root.join("sub/dep.proto")).unwrap(),
            DEP
        );
    }

    #[test]
    fn files_with_the_same_bytes_are_skipped_with_a_note() {
        let source = picked_dir();
        let target = tempfile::tempdir().unwrap();
        let picked = source.path().join("main.proto");
        import_proto(&picked, target.path()).unwrap();

        let again = import_proto(&picked, target.path()).unwrap();

        assert_eq!(again.added, ["main.proto"]);
        assert_eq!(again.warnings.len(), 2);
    }

    #[test]
    fn other_content_is_refused_and_nothing_is_written() {
        let source = picked_dir();
        let target = tempfile::tempdir().unwrap();
        std::fs::create_dir(target.path().join("sub")).unwrap();
        std::fs::write(target.path().join("sub/dep.proto"), "// mine\n").unwrap();

        let error = import_proto(&source.path().join("main.proto"), target.path()).unwrap_err();

        assert_eq!(
            error.message,
            "protos/sub/dep.proto already exists with other content."
        );
        assert_eq!(files_in(target.path()), ["sub/dep.proto"]);
    }

    #[test]
    fn a_broken_file_or_an_import_outside_the_folder_writes_nothing() {
        let source = picked_dir();
        let target = tempfile::tempdir().unwrap();
        let nested = source.path().join("sub/uses_parent.proto");
        std::fs::write(&nested, "syntax = \"proto3\";\nimport \"main.proto\";\n").unwrap();
        std::fs::write(source.path().join("bad.proto"), "message {").unwrap();
        std::fs::write(source.path().join("notes.txt"), DEP).unwrap();

        let outside = import_proto(&nested, target.path()).unwrap_err();
        let broken = import_proto(&source.path().join("bad.proto"), target.path()).unwrap_err();
        let not_proto = import_proto(&source.path().join("notes.txt"), target.path()).unwrap_err();

        assert!(
            outside.message.contains("folder of the picked file"),
            "{outside}"
        );
        assert!(broken.message.starts_with("bad.proto:1:"), "{broken}");
        assert_eq!(not_proto.message, "Pick a .proto file.");
        assert!(files_in(target.path()).is_empty());
    }
}
