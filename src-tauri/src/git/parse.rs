//! Parsers for `git status --porcelain=v2 -z --branch` and `git branch`.

use super::types::{GitBranch, GitFileChange, GitStatus};

/// Parses the NUL-separated porcelain v2 output. Unknown record kinds are ignored.
pub fn parse_status(output: &str) -> GitStatus {
    let mut status = GitStatus {
        git_available: true,
        is_repo: true,
        ..GitStatus::default()
    };
    let mut records = output.split('\0').filter(|r| !r.is_empty());
    while let Some(record) = records.next() {
        if let Some(header) = record.strip_prefix("# ") {
            apply_header(&mut status, header);
            continue;
        }
        let kind = record.as_bytes().first().copied();
        let file = match kind {
            Some(b'1') => change(record, 8),
            Some(b'2') => {
                records.next(); // the original path of a rename
                change(record, 9)
            }
            Some(b'u') => unmerged(record),
            Some(b'?') => Some(GitFileChange {
                path: record[2..].to_string(),
                index: "?".into(),
                worktree: "?".into(),
                conflicted: false,
            }),
            _ => None,
        };
        status.files.extend(file);
    }
    status
}

fn apply_header(status: &mut GitStatus, header: &str) {
    if let Some(name) = header.strip_prefix("branch.head ") {
        status.branch = (name != "(detached)").then(|| name.to_string());
    } else if let Some(name) = header.strip_prefix("branch.upstream ") {
        status.upstream = Some(name.to_string());
    } else if let Some(counts) = header.strip_prefix("branch.ab ") {
        let mut parts = counts.split(' ');
        let number = |p: Option<&str>, sign: char| {
            p.and_then(|p| p.strip_prefix(sign))
                .and_then(|n| n.parse().ok())
                .unwrap_or(0)
        };
        status.ahead = number(parts.next(), '+');
        status.behind = number(parts.next(), '-');
    }
}

/// Ordinary (`1`) and rename (`2`) records: `fields` leading space-separated fields, then the path.
fn change(record: &str, fields: usize) -> Option<GitFileChange> {
    let mut parts = record.splitn(fields + 1, ' ');
    parts.next()?;
    let xy = parts.next()?;
    let path = parts.nth(fields - 2)?;
    let mut letters = xy.chars();
    Some(GitFileChange {
        path: path.to_string(),
        index: letters.next()?.to_string(),
        worktree: letters.next()?.to_string(),
        conflicted: false,
    })
}

fn unmerged(record: &str) -> Option<GitFileChange> {
    let mut parts = record.splitn(11, ' ');
    parts.next()?;
    let xy = parts.next()?;
    let path = parts.nth(8)?;
    let mut letters = xy.chars();
    Some(GitFileChange {
        path: path.to_string(),
        index: letters.next()?.to_string(),
        worktree: letters.next()?.to_string(),
        conflicted: true,
    })
}

/// `git branch --format=%(HEAD)%(refname:short)` lines: `*name` or ` name`.
pub fn parse_branches(output: &str) -> Vec<GitBranch> {
    output
        .lines()
        .filter_map(|line| {
            let current = line.starts_with('*');
            let name = line.get(1..)?.trim();
            // "(HEAD detached at …)" is not a branch you can switch to.
            (!name.is_empty() && !name.starts_with('(')).then(|| GitBranch {
                name: name.to_string(),
                current,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OID: &str = "1111111111111111111111111111111111111111";

    fn one(xy: &str, path: &str) -> String {
        format!("1 {xy} N... 100644 100644 100644 {OID} {OID} {path}\0")
    }

    #[test]
    fn reads_branch_headers() {
        let out = "# branch.oid abc\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0";
        let s = parse_status(out);
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!(s.upstream.as_deref(), Some("origin/main"));
        assert_eq!((s.ahead, s.behind), (2, 1));
        assert!(s.files.is_empty());
    }

    #[test]
    fn detached_head_has_no_branch() {
        assert_eq!(parse_status("# branch.head (detached)\0").branch, None);
    }

    #[test]
    fn reads_changes_with_spaces_in_paths() {
        let out = format!(
            "{}{}? new file.http.yaml\0",
            one("M.", "requests/a b.http.yaml"),
            one(".D", "x.yaml")
        );
        let files = parse_status(&out).files;
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path, "requests/a b.http.yaml");
        assert_eq!(
            (files[0].index.as_str(), files[0].worktree.as_str()),
            ("M", ".")
        );
        assert_eq!(files[1].worktree, "D");
        assert_eq!(
            (files[2].path.as_str(), files[2].index.as_str()),
            ("new file.http.yaml", "?")
        );
    }

    #[test]
    fn rename_record_consumes_its_original_path() {
        let out = format!(
            "2 R. N... 100644 100644 100644 {OID} {OID} R100 new.yaml\0old.yaml\0{}",
            one("M.", "z.yaml")
        );
        let files = parse_status(&out).files;
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "new.yaml");
        assert_eq!(files[1].path, "z.yaml");
    }

    #[test]
    fn unmerged_is_conflicted() {
        let out = format!(
            "u UU N... 100644 100644 100644 100644 {OID} {OID} {OID} requests/c.http.yaml\0"
        );
        let files = parse_status(&out).files;
        assert!(files[0].conflicted);
        assert_eq!(files[0].path, "requests/c.http.yaml");
    }

    #[test]
    fn reads_branches() {
        let out = "*main\n feature/x\n*(HEAD detached at abc)\n";
        let b = parse_branches(out);
        assert_eq!(b.len(), 2);
        assert!(b[0].current && !b[1].current);
        assert_eq!(b[1].name, "feature/x");
    }
}
