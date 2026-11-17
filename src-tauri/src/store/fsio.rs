//! Every filesystem call the store makes (with `paths.rs`). Other store modules only pass
//! paths that came out of `paths::resolve` or were built from the app data dir.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::AppError;

/// Files larger than this are refused before parsing.
pub const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;

const TEMP_PREFIX: &str = ".papan-tmp-";

/// What a directory entry is, without following symlinks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub kind: EntryKind,
}

/// Whether [`write_atomic`] touched the disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    Written,
    Unchanged,
}

fn io_error(path: &Path, source: std::io::Error) -> AppError {
    let detail = format!("{}: {source}", path.display());
    AppError::from(source).with_detail(detail)
}

/// Kind of the entry at `path` (not following symlinks), or `None` when it does not exist.
pub fn entry_kind(path: &Path) -> Option<EntryKind> {
    std::fs::symlink_metadata(path)
        .ok()
        .map(|meta| kind_of(meta.file_type()))
}

fn kind_of(file_type: std::fs::FileType) -> EntryKind {
    if file_type.is_symlink() {
        EntryKind::Symlink
    } else if file_type.is_dir() {
        EntryKind::Dir
    } else if file_type.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}

/// Lists a directory without following symlinks. Names that are not UTF-8 are skipped.
pub fn list_dir(path: &Path) -> Result<Vec<DirEntry>, AppError> {
    let entries = std::fs::read_dir(path).map_err(|source| io_error(path, source))?;
    let listed = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let kind = entry.file_type().map(kind_of).ok()?;
            Some(DirEntry { name, kind })
        })
        .collect();
    Ok(listed)
}

/// Reads a regular file of at most [`MAX_FILE_BYTES`].
pub fn read_bounded(path: &Path) -> Result<Vec<u8>, AppError> {
    if entry_kind(path) != Some(EntryKind::File) {
        return Err(
            AppError::not_found("The file does not exist.").with_detail(path.display().to_string())
        );
    }
    let file = std::fs::File::open(path).map_err(|source| io_error(path, source))?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| io_error(path, source))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(AppError::invalid_input("The file is larger than 5 MiB.")
            .with_detail(path.display().to_string()));
    }
    Ok(bytes)
}

/// Replaces `target` with `bytes` through a temp file in the same directory and a rename.
/// Skipped when the file already holds exactly these bytes.
pub fn write_atomic(target: &Path, bytes: &[u8]) -> Result<WriteOutcome, AppError> {
    if entry_kind(target) == Some(EntryKind::File) {
        if let Ok(current) = read_bounded(target) {
            if current == bytes {
                return Ok(WriteOutcome::Unchanged);
            }
        }
    }
    let parent = target
        .parent()
        .ok_or_else(|| AppError::internal("A file operation failed.").with_detail("no parent"))?;
    let mut temp = tempfile::Builder::new()
        .prefix(TEMP_PREFIX)
        .tempfile_in(parent)
        .map_err(|source| io_error(target, source))?;
    temp.write_all(bytes)
        .and_then(|()| temp.as_file().sync_all())
        .map_err(|source| io_error(target, source))?;
    temp.persist(target)
        .map_err(|error| io_error(target, error.error))?;
    Ok(WriteOutcome::Written)
}

/// Size and modification time of a regular file (not following symlinks).
pub fn file_stamp(path: &Path) -> Option<(u64, Option<SystemTime>)> {
    std::fs::symlink_metadata(path)
        .ok()
        .filter(std::fs::Metadata::is_file)
        .map(|meta| (meta.len(), meta.modified().ok()))
}

/// Removes leftover temp files of an interrupted [`write_atomic`] directly in `dir`.
pub fn sweep_temp_files(dir: &Path) -> usize {
    let stale: Vec<PathBuf> = list_dir(dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| entry.kind == EntryKind::File && entry.name.starts_with(TEMP_PREFIX))
        .map(|entry| dir.join(entry.name))
        .collect();
    stale
        .iter()
        .filter(|path| match remove_file(path) {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(detail = ?error.detail, "could not remove a stale temp file");
                false
            }
        })
        .count()
}

pub fn create_dir(path: &Path) -> Result<(), AppError> {
    std::fs::create_dir(path).map_err(|source| io_error(path, source))
}

pub fn create_dir_all(path: &Path) -> Result<(), AppError> {
    std::fs::create_dir_all(path).map_err(|source| io_error(path, source))
}

pub fn remove_file(path: &Path) -> Result<(), AppError> {
    std::fs::remove_file(path).map_err(|source| io_error(path, source))
}

pub fn remove_dir_all(path: &Path) -> Result<(), AppError> {
    std::fs::remove_dir_all(path).map_err(|source| io_error(path, source))
}

pub fn rename(from: &Path, to: &Path) -> Result<(), AppError> {
    std::fs::rename(from, to).map_err(|source| io_error(from, source))
}

pub fn canonicalize(path: &Path) -> Result<PathBuf, AppError> {
    std::fs::canonicalize(path).map_err(|source| io_error(path, source))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    fn temp_files(dir: &Path) -> Vec<String> {
        list_dir(dir)
            .unwrap()
            .into_iter()
            .map(|entry| entry.name)
            .filter(|name| name.starts_with(TEMP_PREFIX))
            .collect()
    }

    #[test]
    fn write_atomic_creates_file_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.yaml");

        let outcome = write_atomic(&target, b"x: 1\n").unwrap();

        assert_eq!(outcome, WriteOutcome::Written);
        assert_eq!(read_bounded(&target).unwrap(), b"x: 1\n");
        assert!(temp_files(dir.path()).is_empty());
    }

    #[test]
    fn write_atomic_skips_identical_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.yaml");
        write_atomic(&target, b"same\n").unwrap();

        let outcome = write_atomic(&target, b"same\n").unwrap();

        assert_eq!(outcome, WriteOutcome::Unchanged);
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_keeps_old_content_when_directory_is_read_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.yaml");
        write_atomic(&target, b"old\n").unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();

        let result = write_atomic(&target, b"new\n");

        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(result.unwrap_err().code, ErrorCode::Io);
        assert_eq!(read_bounded(&target).unwrap(), b"old\n");
        assert!(temp_files(dir.path()).is_empty());
    }

    #[test]
    fn read_bounded_refuses_files_over_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("big.yaml");
        let file = std::fs::File::create(&target).unwrap();
        file.set_len(MAX_FILE_BYTES + 1).unwrap();

        let error = read_bounded(&target).unwrap_err();

        assert_eq!(error.code, ErrorCode::InvalidInput);
    }

    #[test]
    fn read_bounded_reports_missing_file_as_not_found() {
        let dir = tempfile::tempdir().unwrap();

        let error = read_bounded(&dir.path().join("missing")).unwrap_err();

        assert_eq!(error.code, ErrorCode::NotFound);
    }

    #[test]
    fn list_dir_reports_kinds() {
        let dir = tempfile::tempdir().unwrap();
        create_dir(&dir.path().join("sub")).unwrap();
        write_atomic(&dir.path().join("f"), b"x").unwrap();

        let mut entries = list_dir(dir.path()).unwrap();
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        assert_eq!(
            entries,
            vec![
                DirEntry {
                    name: "f".into(),
                    kind: EntryKind::File
                },
                DirEntry {
                    name: "sub".into(),
                    kind: EntryKind::Dir
                },
            ]
        );
        assert_eq!(entry_kind(&dir.path().join("none")), None);
    }

    #[test]
    fn sweep_removes_only_stale_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        write_atomic(&dir.path().join(".papan-tmp-abc"), b"x").unwrap();
        write_atomic(&dir.path().join("keep.http.yaml"), b"x").unwrap();
        create_dir(&dir.path().join(".papan-tmp-dir")).unwrap();

        let removed = sweep_temp_files(dir.path());

        assert_eq!(removed, 1);
        assert_eq!(entry_kind(&dir.path().join(".papan-tmp-abc")), None);
        assert_eq!(
            entry_kind(&dir.path().join("keep.http.yaml")),
            Some(EntryKind::File)
        );
        assert_eq!(
            entry_kind(&dir.path().join(".papan-tmp-dir")),
            Some(EntryKind::Dir)
        );
    }

    #[test]
    fn file_stamp_reports_regular_files_only() {
        let dir = tempfile::tempdir().unwrap();
        write_atomic(&dir.path().join("f"), b"abc").unwrap();

        assert_eq!(file_stamp(&dir.path().join("f")).unwrap().0, 3);
        assert_eq!(file_stamp(dir.path()), None);
    }

    #[test]
    fn io_failures_carry_the_path_in_detail() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");

        let error = remove_file(&missing).unwrap_err();

        assert_eq!(error.code, ErrorCode::Io);
        assert!(error.detail.unwrap().contains("missing"));
        assert!(rename(&missing, &dir.path().join("b")).is_err());
        assert!(remove_dir_all(&missing).is_err());
        assert!(canonicalize(&missing).is_err());
        assert!(list_dir(&missing).is_err());
    }
}
