//! IPC types for the Git panel.

use serde::{Deserialize, Serialize};
use specta::Type;

/// One changed path. `index` / `worktree` are git's one-letter states (`M`, `A`, `D`, `R`,
/// `?` for untracked, `.` for unchanged).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct GitFileChange {
    pub path: String,
    pub index: String,
    pub worktree: String,
    pub conflicted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
pub struct GitStatus {
    /// `false` when the `git` binary cannot be run.
    pub git_available: bool,
    /// The collection folder is itself a repository root.
    pub is_repo: bool,
    /// Current branch; `None` on a detached HEAD or an empty repo with no commits yet.
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub merging: bool,
    pub files: Vec<GitFileChange>,
    /// Set when the repository's own config names a program Git would run (a shared
    /// folder can carry one). Nothing but `status` runs until it is removed.
    pub unsafe_config: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct GitBranch {
    pub name: String,
    pub current: bool,
}

/// Outcome of pull / push / commit: git's own (redacted) message for the toast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct GitOutput {
    pub message: String,
}

/// Which side wins when resolving a conflicted file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ConflictSide {
    Mine,
    Theirs,
}
