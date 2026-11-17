//! Names we generate (`slugify`, `unique_name`) and the single gate that turns a
//! webview-supplied relative path into a filesystem path (`resolve`). See plan §4.

use std::path::{Path, PathBuf};

use super::fsio::{self, EntryKind};
use crate::error::AppError;

pub const REQUEST_SUFFIX: &str = ".http.yaml";
pub const FOLDER_FILE: &str = "_folder.yaml";

const MAX_SLUG_CHARS: usize = 60;
const MAX_PATH_BYTES: usize = 1024;
const MAX_SEGMENT_BYTES: usize = 255;
const MAX_COLLISION_SUFFIX: u32 = 999;
const FORBIDDEN_CHARS: &[char] = &['<', '>', '"', '|', '?', '*', '\\', ':', '/'];
const RESERVED_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Lowercase ASCII slug of a display name; `untitled` when nothing is left.
pub fn slugify(name: &str) -> String {
    let lower = name.to_lowercase();
    let dashed = lower
        .split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit()))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let cut: String = dashed.chars().take(MAX_SLUG_CHARS).collect();
    let trimmed = cut.trim_matches('-');
    if trimmed.is_empty() {
        "untitled".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Text before the first `.` of a file or directory name.
pub fn stem(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}

fn is_reserved(stem: &str) -> bool {
    RESERVED_NAMES.contains(&stem.to_ascii_lowercase().as_str())
}

/// First free stem among `slug`, `slug-2` … `slug-999`, compared case-insensitively with
/// the stems of `siblings` and with reserved Windows names.
pub fn unique_stem(slug: &str, siblings: &[String]) -> Result<String, AppError> {
    let taken: Vec<String> = siblings
        .iter()
        .map(|name| stem(name).to_ascii_lowercase())
        .collect();
    let is_free = |candidate: &str| {
        let lower = candidate.to_ascii_lowercase();
        !is_reserved(&lower) && !taken.contains(&lower)
    };
    std::iter::once(slug.to_string())
        .chain((2..=MAX_COLLISION_SUFFIX).map(|n| format!("{slug}-{n}")))
        .find(|candidate| is_free(candidate))
        .ok_or_else(|| AppError::invalid_input("Too many items share that name."))
}

/// Whether one path segment is acceptable on every OS (plan §4 rule 2).
pub fn is_valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && !segment.starts_with('.')
        && segment.len() <= MAX_SEGMENT_BYTES
        && !segment
            .chars()
            .any(|c| c.is_control() || FORBIDDEN_CHARS.contains(&c))
        && !segment.ends_with(' ')
        && !segment.ends_with('.')
        && !is_reserved(stem(segment))
}

fn not_allowed(rel: &str) -> AppError {
    AppError::invalid_input("That path is not allowed.").with_detail(format!("{rel:?}"))
}

/// Splits a relative path into checked segments (`""` is the root and has none).
pub fn segments(rel: &str) -> Result<Vec<&str>, AppError> {
    if rel.is_empty() {
        return Ok(Vec::new());
    }
    let bad_text =
        rel.len() > MAX_PATH_BYTES || rel.contains(['\0', '\\', ':']) || rel.starts_with('/');
    if bad_text {
        return Err(not_allowed(rel));
    }
    let parts: Vec<&str> = rel.split('/').collect();
    if parts.iter().all(|part| is_valid_segment(part)) {
        Ok(parts)
    } else {
        Err(not_allowed(rel))
    }
}

/// The only way a webview-supplied path becomes a filesystem path: checks the text,
/// refuses symlinks on the way, and confirms the result stays inside `requests_root`.
pub fn resolve(requests_root: &Path, rel: &str) -> Result<PathBuf, AppError> {
    let parts = segments(rel)?;
    if fsio::entry_kind(requests_root) != Some(EntryKind::Dir) {
        return Err(not_allowed(rel));
    }
    let joined = parts
        .iter()
        .try_fold(requests_root.to_path_buf(), |current, part| {
            let next = current.join(part);
            match fsio::entry_kind(&next) {
                Some(EntryKind::Symlink) => Err(not_allowed(rel)),
                _ => Ok(next),
            }
        })?;
    ensure_inside(requests_root, &joined).map_err(|_| not_allowed(rel))?;
    Ok(joined)
}

/// Defence in depth: the deepest existing ancestor must canonicalise inside the root.
fn ensure_inside(root: &Path, path: &Path) -> Result<(), AppError> {
    let canonical_root = fsio::canonicalize(root)?;
    let existing = path
        .ancestors()
        .find(|ancestor| fsio::entry_kind(ancestor).is_some())
        .unwrap_or(root);
    let canonical = fsio::canonicalize(existing)?;
    if canonical.starts_with(&canonical_root) {
        Ok(())
    } else {
        Err(AppError::invalid_input("That path is not allowed."))
    }
}

/// Joins relative path segments with `/`.
pub fn join_rel(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

/// Parent of a relative path (`""` for top-level items) and the last segment.
pub fn split_rel(rel: &str) -> (&str, &str) {
    rel.rsplit_once('/').unwrap_or(("", rel))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[test]
    fn slugify_table() {
        let long = "a".repeat(200);
        let cases = [
            ("Get Users", "get-users"),
            ("  Login: admin!! ", "login-admin"),
            ("Ünïcödé", "n-c-d"),
            ("用户列表", "untitled"),
            ("🚀", "untitled"),
            ("---", "untitled"),
            ("v2.1 API", "v2-1-api"),
            ("CON", "con"),
        ];
        for (input, expected) in cases {
            assert_eq!(slugify(input), expected, "slugify({input:?})");
        }
        assert_eq!(slugify(&long).len(), 60);
    }

    #[test]
    fn unique_stem_avoids_reserved_names() {
        assert_eq!(unique_stem(&slugify("CON"), &[]).unwrap(), "con-2");
    }

    #[test]
    fn unique_stem_compares_case_insensitively_and_counts_up() {
        let siblings = vec!["Users.http.yaml".to_string(), "users-2".to_string()];

        assert_eq!(unique_stem("users", &siblings).unwrap(), "users-3");
        assert_eq!(unique_stem("orders", &siblings).unwrap(), "orders");
    }

    #[test]
    fn unique_stem_gives_up_after_999() {
        let siblings: Vec<String> = std::iter::once("x".to_string())
            .chain((2..=999).map(|n| format!("x-{n}")))
            .collect();

        let error = unique_stem("x", &siblings).unwrap_err();

        assert_eq!(error.code, ErrorCode::InvalidInput);
    }

    #[test]
    fn resolve_rejects_hostile_paths() {
        let dir = tempfile::tempdir().unwrap();
        let long_segment = "a".repeat(256);
        let long_path = format!("{}/b", "a/".repeat(512));
        let hostile = [
            "..",
            "a/../b",
            "/etc/passwd",
            "C:\\x",
            "C:/x",
            "a\\b",
            "\\\\server\\share",
            "a//b",
            "./a",
            "a/.",
            "nul.http.yaml",
            "Com1",
            "a:b",
            "a?",
            "trailing. ",
            "a\0b",
            "a\u{1}b",
            long_segment.as_str(),
            long_path.as_str(),
            "a/",
            "trailing.",
            ".git",
            "a/.hidden.http.yaml",
            ".papan-tmp-x",
        ];
        for rel in hostile {
            let error = resolve(dir.path(), rel).unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidInput, "{rel:?}");
            assert_eq!(error.message, "That path is not allowed.");
        }
    }

    #[test]
    fn resolve_accepts_plain_relative_paths() {
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(
            resolve(dir.path(), "a/b/c.http.yaml").unwrap(),
            dir.path().join("a/b/c.http.yaml")
        );
        assert_eq!(resolve(dir.path(), "").unwrap(), dir.path());
    }

    #[test]
    fn resolve_rejects_a_missing_root() {
        let dir = tempfile::tempdir().unwrap();

        assert!(resolve(&dir.path().join("missing"), "a").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_rejects_symlinks_on_the_way() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();

        assert!(resolve(dir.path(), "link").is_err());
        assert!(resolve(dir.path(), "link/x.http.yaml").is_err());
    }

    #[test]
    fn rel_helpers_split_and_join() {
        assert_eq!(join_rel("", "a"), "a");
        assert_eq!(join_rel("a", "b"), "a/b");
        assert_eq!(split_rel("a/b/c"), ("a/b", "c"));
        assert_eq!(split_rel("c"), ("", "c"));
        assert_eq!(stem("login.http.yaml"), "login");
    }
}
