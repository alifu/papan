//! Git operations on a collection folder. Plain Rust: every function takes the repo root.

use std::path::{Component, Path};

use super::parse::{parse_branches, parse_status};
use super::runner::{reject_option_like, run, run_ok, LOCAL_TIMEOUT, NETWORK_TIMEOUT};
use super::types::{ConflictSide, GitBranch, GitFileChange, GitOutput, GitStatus};
use crate::error::{AppError, ErrorCode};

/// Local-only files that must not be shared.
pub const GITIGNORE: &str = "# Papan: local-only files\n.papan-tmp-*\n.DS_Store\nThumbs.db\n";

/// Whether `root` is the top level of its own repository.
fn is_repo_root(root: &Path) -> Result<bool, AppError> {
    let found = run(root, &["rev-parse", "--show-prefix"], LOCAL_TIMEOUT)?;
    Ok(found.success && found.stdout.trim().is_empty())
}

pub fn status(root: &Path) -> Result<GitStatus, AppError> {
    let is_repo = match is_repo_root(root) {
        Ok(is_repo) => is_repo,
        Err(error) if error.code == ErrorCode::NotFound => return Ok(GitStatus::default()),
        Err(error) => return Err(error),
    };
    if !is_repo {
        return Ok(GitStatus {
            git_available: true,
            ..GitStatus::default()
        });
    }
    let out = run_ok(
        root,
        &[
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=all",
        ],
        LOCAL_TIMEOUT,
    )?;
    let merging = run(
        root,
        &["rev-parse", "-q", "--verify", "MERGE_HEAD"],
        LOCAL_TIMEOUT,
    )?
    .success;
    Ok(GitStatus {
        merging,
        ..parse_status(&out.stdout)
    })
}

fn require_repo(root: &Path) -> Result<GitStatus, AppError> {
    let current = status(root)?;
    if !current.git_available {
        return Err(AppError::not_found(
            "Git could not be started. Is it installed?",
        ));
    }
    if !current.is_repo {
        return Err(AppError::invalid_input(
            "This collection is not a Git repository yet.",
        ));
    }
    Ok(current)
}

/// Creates the generated `.gitignore` unless one exists.
pub fn ensure_gitignore(root: &Path) -> Result<(), AppError> {
    let path = root.join(".gitignore");
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut file) => {
            std::io::Write::write_all(&mut file, GITIGNORE.as_bytes())?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub fn init(root: &Path) -> Result<GitStatus, AppError> {
    let current = status(root)?;
    if !current.git_available {
        return Err(AppError::not_found(
            "Git could not be started. Is it installed?",
        ));
    }
    if !current.is_repo {
        run_ok(root, &["init"], LOCAL_TIMEOUT)?;
    }
    ensure_gitignore(root)?;
    status(root)
}

/// The folder name a clone of `url` gets: the last path segment without `.git`.
pub fn repo_dir_name(url: &str) -> Result<String, AppError> {
    let trimmed = url.trim().trim_end_matches('/');
    let last = trimmed.rsplit(['/', ':', '\\']).next().unwrap_or("");
    let name = last.strip_suffix(".git").unwrap_or(last);
    let valid = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || "-_. ".contains(c));
    if valid {
        Ok(name.to_string())
    } else {
        Err(AppError::invalid_input(
            "Could not work out a folder name from that URL.",
        ))
    }
}

/// Clones into `parent/<name>`; the folder must not exist. Returns the new folder.
pub fn clone(url: &str, parent: &Path) -> Result<std::path::PathBuf, AppError> {
    let url = url.trim();
    reject_option_like("The repository URL", url)?;
    let target = parent.join(repo_dir_name(url)?);
    if target.exists() {
        return Err(AppError::invalid_input(format!(
            "“{}” already exists in that folder.",
            target
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("The folder")
        )));
    }
    let target_arg = target.to_string_lossy().into_owned();
    run_ok(parent, &["clone", "--", url, &target_arg], NETWORK_TIMEOUT)?;
    Ok(target)
}

/// A repo-relative path from the UI: no absolute paths, no `..`, nothing option-like.
pub fn safe_rel(path: &str) -> Result<&str, AppError> {
    let candidate = Path::new(path);
    let clean = !path.is_empty()
        && !path.starts_with('-')
        && !path.contains('\0')
        && !candidate.is_absolute()
        && candidate
            .components()
            .all(|c| matches!(c, Component::Normal(_)));
    if clean {
        Ok(path)
    } else {
        Err(AppError::invalid_input(
            "That path is not inside the collection.",
        ))
    }
}

fn find_change<'a>(files: &'a [GitFileChange], path: &str) -> Result<&'a GitFileChange, AppError> {
    files
        .iter()
        .find(|f| f.path == path)
        .ok_or_else(|| AppError::not_found("That file has no changes."))
}

pub fn diff(root: &Path, path: &str) -> Result<String, AppError> {
    let path = safe_rel(path)?;
    let current = require_repo(root)?;
    let change = find_change(&current.files, path)?;
    let has_head = run(
        root,
        &["rev-parse", "-q", "--verify", "HEAD"],
        LOCAL_TIMEOUT,
    )?
    .success;
    let result = if change.index == "?" {
        // Exit code 1 only means "there are differences".
        run(
            root,
            &["diff", "--no-index", "--no-color", "--", "/dev/null", path],
            LOCAL_TIMEOUT,
        )?
    } else if change.conflicted || !has_head {
        run_ok(root, &["diff", "--no-color", "--", path], LOCAL_TIMEOUT)?
    } else {
        run_ok(
            root,
            &["diff", "--no-color", "HEAD", "--", path],
            LOCAL_TIMEOUT,
        )?
    };
    Ok(result.stdout)
}

pub fn commit(root: &Path, message: &str) -> Result<GitOutput, AppError> {
    let message = message.trim();
    if message.is_empty() {
        return Err(AppError::invalid_input("Write a commit message first."));
    }
    let current = require_repo(root)?;
    if current.files.iter().any(|f| f.conflicted) {
        return Err(AppError::invalid_input(
            "Resolve the conflicted files before committing.",
        ));
    }
    if current.files.is_empty() && !current.merging {
        return Err(AppError::invalid_input("There is nothing to commit."));
    }
    ensure_gitignore(root)?;
    run_ok(root, &["add", "-A"], LOCAL_TIMEOUT)?;
    let done = run_ok(root, &["commit", "-m", message], LOCAL_TIMEOUT)?;
    Ok(GitOutput {
        message: done
            .stdout
            .lines()
            .next()
            .unwrap_or("Committed.")
            .to_string(),
    })
}

pub fn pull(root: &Path) -> Result<GitOutput, AppError> {
    require_repo(root)?;
    let result = run(root, &["pull", "--no-rebase", "--no-edit"], NETWORK_TIMEOUT)?;
    if result.success {
        return Ok(GitOutput {
            message: last_line(&result.stdout, "Pulled."),
        });
    }
    if status(root)?.files.iter().any(|f| f.conflicted) {
        return Ok(GitOutput {
            message: "The pull stopped on conflicts. Resolve each file, then commit.".into(),
        });
    }
    let text = result.stderr.trim();
    Err(AppError::invalid_input(if text.is_empty() {
        result.stdout.trim()
    } else {
        text
    }))
}

pub fn push(root: &Path) -> Result<GitOutput, AppError> {
    let current = require_repo(root)?;
    let done = match (&current.upstream, &current.branch) {
        (None, Some(branch)) => run_ok(
            root,
            &["push", "--set-upstream", "origin", branch],
            NETWORK_TIMEOUT,
        )?,
        _ => run_ok(root, &["push"], NETWORK_TIMEOUT)?,
    };
    // git writes push progress to stderr.
    Ok(GitOutput {
        message: last_line(&done.stderr, "Pushed."),
    })
}

fn last_line(text: &str, fallback: &str) -> String {
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map_or_else(|| fallback.to_string(), |l| l.trim().to_string())
}

pub fn branches(root: &Path) -> Result<Vec<GitBranch>, AppError> {
    require_repo(root)?;
    let out = run_ok(
        root,
        &["branch", "--format=%(HEAD)%(refname:short)"],
        LOCAL_TIMEOUT,
    )?;
    Ok(parse_branches(&out.stdout))
}

pub fn switch(root: &Path, name: &str) -> Result<GitStatus, AppError> {
    reject_option_like("The branch name", name)?;
    require_repo(root)?;
    run_ok(root, &["switch", name], LOCAL_TIMEOUT)?;
    status(root)
}

pub fn create_branch(root: &Path, name: &str) -> Result<GitStatus, AppError> {
    let name = name.trim();
    reject_option_like("The branch name", name)?;
    require_repo(root)?;
    let valid = run(root, &["check-ref-format", "--branch", name], LOCAL_TIMEOUT)?;
    if !valid.success {
        return Err(AppError::invalid_input("That is not a valid branch name."));
    }
    run_ok(root, &["switch", "-c", name], LOCAL_TIMEOUT)?;
    status(root)
}

/// Keeps one side of a conflicted file and stages it. "Mine" is the current branch.
pub fn resolve(root: &Path, path: &str, side: ConflictSide) -> Result<GitStatus, AppError> {
    let path = safe_rel(path)?;
    let current = require_repo(root)?;
    if !find_change(&current.files, path)?.conflicted {
        return Err(AppError::invalid_input("That file is not in conflict."));
    }
    let flag = match side {
        ConflictSide::Mine => "--ours",
        ConflictSide::Theirs => "--theirs",
    };
    run_ok(root, &["checkout", flag, "--", path], LOCAL_TIMEOUT)?;
    run_ok(root, &["add", "--", path], LOCAL_TIMEOUT)?;
    status(root)
}
