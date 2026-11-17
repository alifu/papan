use super::*;
use crate::error::ErrorCode;

const HERE: &str = "collection:a";
const ELSEWHERE: &str = "collection:b";

fn setup() -> (tempfile::TempDir, Grants, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("upload.bin");
    std::fs::write(&file, [1u8; 10]).unwrap();
    let grants = Grants::load(dir.path());
    (dir, grants, file)
}

#[test]
fn scopes_follow_the_location() {
    let scratch = RequestLocation::Scratch { id: "s1".into() };
    let collection = RequestLocation::Collection {
        collection_id: "c1".into(),
        path: "x.http.yaml".into(),
    };

    assert_eq!(grant_scope(&scratch), "scratch:s1");
    assert_eq!(grant_scope(&collection), "collection:c1");
}

#[test]
fn only_files_granted_to_the_same_scope_pass_and_grants_survive_a_reload() {
    let (dir, grants, file) = setup();
    let before = grants.check(HERE, &file).unwrap_err();

    let picked = grants.grant(HERE, &file).unwrap();
    let reloaded = Grants::load(dir.path());

    assert_eq!(before.code, ErrorCode::InvalidInput);
    assert!(before.message.contains("upload.bin"));
    assert_eq!(
        (picked.name.as_str(), picked.size_bytes),
        ("upload.bin", 10)
    );
    assert!(reloaded.check(HERE, &file).is_ok());
    assert!(reloaded.check(ELSEWHERE, &file).is_err());
    std::fs::remove_file(&file).unwrap();
    assert!(reloaded.check(HERE, &file).is_err());
}

#[test]
fn carrying_moves_grants_to_the_new_scope() {
    let (_dir, grants, file) = setup();
    grants.grant("scratch:s1", &file).unwrap();

    grants.carry("scratch:s1", HERE).unwrap();
    grants.carry("scratch:none", ELSEWHERE).unwrap();

    assert!(grants.check(HERE, &file).is_ok());
    assert!(grants.check("scratch:s1", &file).is_err());
    assert!(grants.check(ELSEWHERE, &file).is_err());
}

#[cfg(unix)]
#[test]
fn a_symlink_to_an_ungranted_target_is_refused() {
    let (dir, grants, file) = setup();
    let secret = dir.path().join("secret");
    std::fs::write(&secret, b"s").unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&secret, &link).unwrap();
    grants.grant(HERE, &file).unwrap();

    assert!(grants.check(HERE, &link).is_err());
}

#[test]
fn grants_are_capped_and_malformed_files_count_as_empty() {
    let (dir, grants, _) = setup();
    let files: Vec<PathBuf> = (0..=MAX_GRANTS)
        .map(|n| {
            let path = dir.path().join(format!("f{n}"));
            std::fs::write(&path, b"x").unwrap();
            path
        })
        .collect();
    files.iter().for_each(|path| {
        grants.grant(HERE, path).unwrap();
    });

    assert_eq!(grants.lock().len(), MAX_GRANTS);
    assert!(grants.check(HERE, &files[0]).is_err());
    std::fs::write(dir.path().join(GRANTS_FILE), "{").unwrap();
    assert!(Grants::load(dir.path()).lock().is_empty());
    assert!(grants.grant(HERE, dir.path()).is_err());
}
