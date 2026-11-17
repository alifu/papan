use super::*;
use crate::error::ErrorCode;

fn put(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fsio::create_dir_all(path.parent().unwrap()).unwrap();
    fsio::write_atomic(&path, text.as_bytes()).unwrap();
}

fn read(root: &Path, rel: &str) -> String {
    String::from_utf8(fsio::read_bounded(&root.join(rel)).unwrap()).unwrap()
}

#[test]
fn copies_every_proto_under_the_same_relative_names() {
    let dir = tempfile::tempdir().unwrap();
    let (from, to) = (dir.path().join("from"), dir.path().join("to/protos"));
    put(&from, "a.proto", "A");
    put(&from, "x/y/b.proto", "B");
    put(&from, "notes.txt", "ignored");

    let copied = copy_protos(&from, &to).unwrap();

    assert_eq!(copied, 2);
    assert_eq!(read(&to, "a.proto"), "A");
    assert_eq!(read(&to, "x/y/b.proto"), "B");
    assert!(!to.join("notes.txt").exists());
}

#[test]
fn identical_existing_files_are_skipped_and_other_content_refuses_everything() {
    let dir = tempfile::tempdir().unwrap();
    let (from, to) = (dir.path().join("from"), dir.path().join("protos"));
    put(&from, "a.proto", "A");
    put(&from, "b.proto", "NEW");
    put(&to, "a.proto", "A");
    put(&to, "b.proto", "OLD");

    let error = copy_protos(&from, &to).unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert_eq!(
        error.message,
        "protos/b.proto already exists with other content."
    );
    assert_eq!(read(&to, "b.proto"), "OLD");
}

#[test]
fn nothing_is_written_when_a_later_file_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let (from, to) = (dir.path().join("from"), dir.path().join("protos"));
    put(&from, "a.proto", "A");
    put(&from, "z.proto", "NEW");
    put(&to, "z.proto", "OLD");

    assert!(copy_protos(&from, &to).is_err());

    assert!(!to.join("a.proto").exists());
}

#[test]
fn a_missing_source_copies_nothing_and_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let to = dir.path().join("protos");

    let copied = copy_protos(&dir.path().join("none"), &to).unwrap();

    assert_eq!(copied, 0);
    assert!(!to.exists());
}

#[cfg(unix)]
#[test]
fn symlinks_in_the_source_are_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let (from, to) = (dir.path().join("from"), dir.path().join("protos"));
    put(&from, "a.proto", "A");
    put(dir.path(), "secret.proto", "S");
    std::os::unix::fs::symlink(dir.path().join("secret.proto"), from.join("link.proto")).unwrap();

    copy_protos(&from, &to).unwrap();

    assert!(to.join("a.proto").exists());
    assert!(!to.join("link.proto").exists());
}

#[cfg(unix)]
#[test]
fn a_symlinked_target_folder_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (from, outside) = (dir.path().join("from"), dir.path().join("outside"));
    put(&from, "a.proto", "A");
    fsio::create_dir_all(&outside).unwrap();
    let to = dir.path().join("protos");
    std::os::unix::fs::symlink(&outside, &to).unwrap();

    assert!(copy_protos(&from, &to).is_err());
    assert!(!outside.join("a.proto").exists());
}

#[test]
fn too_many_files_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let from = dir.path().join("from");
    for n in 0..=MAX_COPIED_FILES {
        put(&from, &format!("p{n}.proto"), "x");
    }

    let error = copy_protos(&from, &dir.path().join("protos")).unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
}
