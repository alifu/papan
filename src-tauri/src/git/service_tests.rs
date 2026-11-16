//! Integration tests on temp repos with a local bare remote.

use std::fs;
use std::path::{Path, PathBuf};

use super::runner::{run_ok, LOCAL_TIMEOUT};
use super::service::*;
use super::types::ConflictSide;
use crate::error::ErrorCode;

fn git(dir: &Path, args: &[&str]) {
    run_ok(dir, args, LOCAL_TIMEOUT).unwrap();
}

fn identify(dir: &Path) {
    git(dir, &["config", "user.name", "Tester"]);
    git(dir, &["config", "user.email", "t@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    // Windows runners default to autocrlf=true, which turns "\n" into "\r\n" in the checkout.
    git(dir, &["config", "core.autocrlf", "false"]);
}

/// A bare remote seeded with one commit, plus two clones of it.
struct World {
    _tmp: tempfile::TempDir,
    remote: PathBuf,
    a: PathBuf,
    b: PathBuf,
}

fn world() -> World {
    let tmp = tempfile::tempdir().unwrap();
    let remote = tmp.path().join("remote.git");
    fs::create_dir(&remote).unwrap();
    git(&remote, &["init", "--bare"]);
    let seed = tmp.path().join("seed");
    fs::create_dir(&seed).unwrap();
    git(&seed, &["init"]);
    identify(&seed);
    fs::write(seed.join("shared.yaml"), "name: one\n").unwrap();
    commit(&seed, "seed");
    git(
        &seed,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&seed, &["push", "--set-upstream", "origin", "HEAD"]);
    git(
        &remote,
        &[
            "symbolic-ref",
            "HEAD",
            &format!("refs/heads/{}", current_branch(&seed)),
        ],
    );
    let url = remote.to_str().unwrap();
    let parent_a = tmp.path().join("A");
    let parent_b = tmp.path().join("B");
    fs::create_dir(&parent_a).unwrap();
    fs::create_dir(&parent_b).unwrap();
    let a = clone(url, &parent_a).unwrap();
    let b = clone(url, &parent_b).unwrap();
    World {
        _tmp: tmp,
        remote,
        a,
        b,
    }
}

fn current_branch(dir: &Path) -> String {
    status(dir).unwrap().branch.unwrap()
}

fn commit(dir: &Path, message: &str) {
    super::service::commit(dir, message).unwrap();
}

#[test]
fn clone_needs_a_free_folder() {
    let w = world();
    assert!(w.a.join("shared.yaml").exists());
    // Cloning again into the same parent finds the folder taken.
    assert!(clone(w.remote.to_str().unwrap(), w.a.parent().unwrap()).is_err());
}

#[test]
fn clone_refuses_option_like_urls() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(clone("--upload-pack=touch pwned", tmp.path()).is_err());
    assert!(clone("ext::sh -c touch% pwned", tmp.path()).is_err());
    assert!(!tmp.path().join("pwned").exists());
}

#[test]
fn derives_folder_names() {
    assert_eq!(
        repo_dir_name("https://github.com/a/api.git").unwrap(),
        "api"
    );
    assert_eq!(
        repo_dir_name("git@github.com:a/my-api.git/").unwrap(),
        "my-api"
    );
    assert!(repo_dir_name("https://x/..").is_err());
    assert!(repo_dir_name("").is_err());
}

#[test]
fn init_creates_repo_and_gitignore_once() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(!status(tmp.path()).unwrap().is_repo);
    let s = init(tmp.path()).unwrap();
    assert!(s.is_repo && s.git_available);
    let ignore = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
    assert_eq!(ignore, GITIGNORE);
    fs::write(tmp.path().join(".gitignore"), "custom\n").unwrap();
    init(tmp.path()).unwrap();
    assert_eq!(
        fs::read_to_string(tmp.path().join(".gitignore")).unwrap(),
        "custom\n"
    );
}

#[test]
fn a_folder_inside_another_repo_is_not_a_repo_root() {
    let tmp = tempfile::tempdir().unwrap();
    git(tmp.path(), &["init"]);
    let inner = tmp.path().join("collection");
    fs::create_dir(&inner).unwrap();
    assert!(!status(&inner).unwrap().is_repo);
    assert!(init(&inner).unwrap().is_repo);
}

#[test]
fn status_diff_and_commit() {
    let w = world();
    identify(&w.a);
    fs::write(w.a.join("shared.yaml"), "name: two\n").unwrap();
    fs::write(w.a.join("new file.yaml"), "x: 1\n").unwrap();
    fs::write(w.a.join(".papan-tmp-1"), "junk").unwrap();
    init(&w.a).unwrap(); // writes .gitignore
    let s = status(&w.a).unwrap();
    let paths: Vec<_> = s.files.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(&"shared.yaml") && paths.contains(&"new file.yaml"));
    assert!(!paths.contains(&".papan-tmp-1"));
    assert!(diff(&w.a, "shared.yaml").unwrap().contains("+name: two"));
    assert!(diff(&w.a, "new file.yaml").unwrap().contains("+x: 1"));
    assert!(diff(&w.a, "../x").is_err());
    assert!(commit_err(&w.a, "  ").message.contains("message"));
    commit(&w.a, "change");
    assert!(status(&w.a).unwrap().files.is_empty());
    assert_eq!(
        commit_err(&w.a, "again").message,
        "There is nothing to commit."
    );
}

fn commit_err(dir: &Path, message: &str) -> crate::error::AppError {
    super::service::commit(dir, message).unwrap_err()
}

#[test]
fn push_then_pull_fast_forward() {
    let w = world();
    identify(&w.a);
    identify(&w.b);
    fs::write(w.a.join("shared.yaml"), "name: from a\n").unwrap();
    commit(&w.a, "a");
    push(&w.a).unwrap();
    assert_eq!(status(&w.a).unwrap().ahead, 0);
    git(&w.b, &["fetch"]);
    assert_eq!(status(&w.b).unwrap().behind, 1);
    pull(&w.b).unwrap();
    assert_eq!(
        fs::read_to_string(w.b.join("shared.yaml")).unwrap(),
        "name: from a\n"
    );
}

#[test]
fn pull_conflict_then_take_mine_and_theirs() {
    let w = world();
    identify(&w.a);
    identify(&w.b);
    fs::write(w.a.join("shared.yaml"), "name: a\n").unwrap();
    commit(&w.a, "a");
    push(&w.a).unwrap();
    fs::write(w.b.join("shared.yaml"), "name: b\n").unwrap();
    commit(&w.b, "b");

    let out = pull(&w.b).unwrap();
    assert!(out.message.contains("conflict"));
    let s = status(&w.b).unwrap();
    assert!(s.merging);
    assert!(s
        .files
        .iter()
        .any(|f| f.conflicted && f.path == "shared.yaml"));
    assert!(commit_err(&w.b, "too soon").message.contains("Resolve"));
    assert!(resolve(&w.b, "../shared.yaml", ConflictSide::Mine).is_err());

    resolve(&w.b, "shared.yaml", ConflictSide::Theirs).unwrap();
    assert_eq!(
        fs::read_to_string(w.b.join("shared.yaml")).unwrap(),
        "name: a\n"
    );
    commit(&w.b, "merge");
    assert!(!status(&w.b).unwrap().merging);
    push(&w.b).unwrap();

    // Mine: a second round on the other clone.
    fs::write(w.b.join("shared.yaml"), "name: b2\n").unwrap();
    commit(&w.b, "b2");
    push(&w.b).unwrap();
    fs::write(w.a.join("shared.yaml"), "name: a2\n").unwrap();
    commit(&w.a, "a2");
    pull(&w.a).unwrap();
    resolve(&w.a, "shared.yaml", ConflictSide::Mine).unwrap();
    assert_eq!(
        fs::read_to_string(w.a.join("shared.yaml")).unwrap(),
        "name: a2\n"
    );
}

#[test]
fn branches_switch_and_create() {
    let w = world();
    let main = current_branch(&w.a);
    create_branch(&w.a, "feature/x").unwrap();
    assert_eq!(current_branch(&w.a), "feature/x");
    let list = branches(&w.a).unwrap();
    assert_eq!(list.len(), 2);
    assert!(list.iter().any(|b| b.current && b.name == "feature/x"));
    switch(&w.a, &main).unwrap();
    assert_eq!(current_branch(&w.a), main);
    assert!(create_branch(&w.a, "bad name").is_err());
    assert!(create_branch(&w.a, "-c").is_err());
    assert!(switch(&w.a, "--detach").is_err());
}

#[test]
fn push_a_new_branch_sets_its_upstream() {
    let w = world();
    create_branch(&w.a, "topic").unwrap();
    push(&w.a).unwrap();
    assert_eq!(
        status(&w.a).unwrap().upstream.as_deref(),
        Some("origin/topic")
    );
}

#[test]
fn operations_need_a_repo() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(pull(tmp.path()).unwrap_err().code, ErrorCode::InvalidInput);
    assert!(branches(tmp.path()).is_err());
}

#[test]
fn a_repo_config_that_runs_a_program_blocks_git_work() {
    let w = world();
    git(&w.a, &["config", "core.sshCommand", "touch pwned"]);
    let s = status(&w.a).unwrap();
    assert_eq!(s.unsafe_config.as_deref(), Some("core.sshcommand"));
    assert!(s.files.is_empty());
    assert!(pull(&w.a).unwrap_err().message.contains("core.sshcommand"));
    assert!(!w.a.join("pwned").exists());
}

#[test]
fn hooks_of_the_repository_do_not_run() {
    let w = world();
    identify(&w.a);
    let hooks = w.a.join(".git/hooks");
    fs::create_dir_all(&hooks).unwrap();
    let hook = hooks.join("pre-commit");
    fs::write(&hook, "#!/bin/sh\ntouch hook-ran\n").unwrap();
    #[cfg(unix)]
    fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    fs::write(w.a.join("shared.yaml"), "name: hooked\n").unwrap();
    commit(&w.a, "go");
    assert!(!w.a.join("hook-ran").exists());
}

#[test]
fn other_transports_are_refused_and_a_failed_clone_leaves_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(clone("foo::bar", tmp.path()).is_err());
    let missing = tmp.path().join("nope.git");
    assert!(clone(missing.to_str().unwrap(), tmp.path()).is_err());
    assert!(!tmp.path().join("nope").exists());
}

#[test]
fn errors_never_carry_url_credentials() {
    let tmp = tempfile::tempdir().unwrap();
    let error = clone("https://user:s3cret@127.0.0.1:1/x.git", tmp.path()).unwrap_err();
    let all = format!("{} {:?}", error.message, error.detail);
    assert!(!all.contains("s3cret"), "{all}");
}

#[test]
fn delete_modify_conflicts_can_be_resolved() {
    let w = world();
    identify(&w.a);
    identify(&w.b);
    git(&w.a, &["rm", "-q", "shared.yaml"]);
    commit(&w.a, "delete");
    push(&w.a).unwrap();
    fs::write(w.b.join("shared.yaml"), "name: kept\n").unwrap();
    commit(&w.b, "modify");
    pull(&w.b).unwrap();
    let s = status(&w.b).unwrap();
    assert!(s.files.iter().any(|f| f.conflicted));
    // Theirs deleted the file: taking theirs removes it, taking mine keeps it.
    resolve(&w.b, "shared.yaml", ConflictSide::Theirs).unwrap();
    assert!(!w.b.join("shared.yaml").exists());
    assert!(!status(&w.b).unwrap().files.iter().any(|f| f.conflicted));
}

#[test]
fn diff_works_for_staged_files_in_an_empty_repo_and_reports_errors() {
    let tmp = tempfile::tempdir().unwrap();
    init(tmp.path()).unwrap();
    fs::write(tmp.path().join("a.yaml"), "x: 1\n").unwrap();
    git(tmp.path(), &["add", "a.yaml"]);
    assert!(diff(tmp.path(), "a.yaml").unwrap().contains("+x: 1"));
    assert!(diff(tmp.path(), "missing.yaml").is_err());
}

#[test]
fn push_explains_a_missing_remote_and_a_detached_head() {
    let tmp = tempfile::tempdir().unwrap();
    init(tmp.path()).unwrap();
    identify(tmp.path());
    fs::write(tmp.path().join("a.yaml"), "x: 1\n").unwrap();
    commit(tmp.path(), "first");
    assert!(push(tmp.path()).unwrap_err().message.contains("no remote"));
    git(tmp.path(), &["checkout", "--detach"]);
    assert!(push(tmp.path()).unwrap_err().message.contains("branch"));
}
