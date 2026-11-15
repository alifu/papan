//! Size, depth and path-safety limits checked before and during mapping (plan 09 §4).
//! The depth pre-scan runs on raw bytes before serde, so a deeply nested file never
//! reaches a recursive parser.

use std::path::Path;

use crate::error::AppError;
use crate::store::fsio;

pub use crate::store::collection::MAX_DEPTH as MAX_FOLDER_DEPTH;
pub use crate::store::env_model::{MAX_VALUE_BYTES, MAX_VARIABLES};
pub use crate::store::environments::MAX_ENVIRONMENTS;

pub const MAX_FILES: usize = 20;
pub const MAX_JSON_DEPTH: usize = 64;
pub const MAX_ITEMS: usize = 5000;
pub const MAX_NAME_CHARS: usize = 100;
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";
const UNNAMED_FILE: &str = "file";

/// `invalid_input` naming the file (name only) and the problem.
pub fn file_error(file: &str, problem: &str) -> AppError {
    AppError::invalid_input(format!("{file}: {problem}"))
}

/// The file name of `path` for messages; never the directories above it.
pub fn file_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map_or_else(|| UNNAMED_FILE.to_string(), str::to_string)
}

pub fn check_file_count(count: usize) -> Result<(), AppError> {
    if count > MAX_FILES {
        return Err(AppError::invalid_input(format!(
            "Import at most {MAX_FILES} files at a time."
        )));
    }
    Ok(())
}

/// Reads one picked file: regular files only (no symlinks), at most 5 MiB.
pub fn read_file(path: &Path) -> Result<Vec<u8>, AppError> {
    let file = file_label(path);
    fsio::read_bounded(path).map_err(|error| {
        // The detail holds the full path: it is dropped so the error names the file only.
        AppError::new(error.code, format!("{file}: {}", error.message))
    })
}

/// Checks nesting depth on raw bytes, outside strings.
pub fn prescan_depth(file: &str, bytes: &[u8]) -> Result<(), AppError> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for &byte in bytes {
        if in_string {
            match (escaped, byte) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > MAX_JSON_DEPTH {
                    return Err(file_error(
                        file,
                        &format!("JSON is nested deeper than {MAX_JSON_DEPTH} levels."),
                    ));
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

/// UTF-8 text without a leading byte order mark.
pub fn decode_text<'a>(file: &str, bytes: &'a [u8]) -> Result<&'a str, AppError> {
    let body = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
    std::str::from_utf8(body).map_err(|_| file_error(file, "is not valid UTF-8 text."))
}

/// Pre-scans and parses one file into a JSON value.
pub fn parse_json(file: &str, bytes: &[u8]) -> Result<serde_json::Value, AppError> {
    let text = decode_text(file, bytes)?;
    prescan_depth(file, text.as_bytes())?;
    serde_json::from_str(text).map_err(|error| {
        file_error(
            file,
            &format!(
                "is not valid JSON (line {}, column {}).",
                error.line(),
                error.column()
            ),
        )
    })
}

/// Bidi controls and zero-width characters: they can disguise a name.
fn is_invisible_format(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200B}'..='\u{200F}' | '\u{FEFF}')
}

/// A display name: trimmed, control characters removed, at most 100 characters,
/// `fallback` when nothing is left. The bool says whether the name was cut to length.
pub fn clean_display_name(raw: &str, fallback: &str) -> (String, bool) {
    let visible: String = raw
        .chars()
        .filter(|c| !c.is_control() && !is_invisible_format(*c))
        .collect();
    let trimmed = visible.trim();
    let was_cut = trimmed.chars().count() > MAX_NAME_CHARS;
    let cut: String = trimmed.chars().take(MAX_NAME_CHARS).collect();
    let cut = cut.trim_end();
    if cut.is_empty() {
        (fallback.to_string(), false)
    } else {
        (cut.to_string(), was_cut)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prescan_counts_nesting_outside_strings() {
        let deep = format!("{}{}", "[".repeat(65), "]".repeat(65));
        let ok = format!("{}{}", "[".repeat(64), "]".repeat(64));
        let in_string = format!("{{\"a\": \"{}\\\"{}\"}}", "[".repeat(200), "{".repeat(200));

        let error = prescan_depth("a.json", deep.as_bytes()).unwrap_err();

        assert!(error.message.starts_with("a.json: "), "{}", error.message);
        assert!(error.message.contains("64"));
        assert!(prescan_depth("a.json", ok.as_bytes()).is_ok());
        assert!(prescan_depth("a.json", in_string.as_bytes()).is_ok());
    }

    #[test]
    fn decode_strips_bom_and_rejects_invalid_utf8() {
        assert_eq!(decode_text("a", b"\xEF\xBB\xBF{}").unwrap(), "{}");
        let error = decode_text("a.json", b"\xff\xfe").unwrap_err();
        assert_eq!(error.message, "a.json: is not valid UTF-8 text.");
    }

    #[test]
    fn parse_json_names_the_file_and_position() {
        let error = parse_json("x.json", b"{\n  nope").unwrap_err();
        assert!(error
            .message
            .starts_with("x.json: is not valid JSON (line 2"));
        assert_eq!(parse_json("x", b"[1]").unwrap(), serde_json::json!([1]));
    }

    #[test]
    fn file_label_is_the_name_only() {
        assert_eq!(file_label(Path::new("/home/u/secret/dev.json")), "dev.json");
        assert_eq!(file_label(Path::new("/")), "file");
    }

    #[test]
    fn file_count_is_capped() {
        assert!(check_file_count(MAX_FILES).is_ok());
        assert!(check_file_count(MAX_FILES + 1).is_err());
    }

    #[test]
    fn read_file_refuses_large_files_and_symlinks_naming_the_file_only() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("big.json");
        std::fs::write(&big, vec![b' '; 5 * 1024 * 1024 + 1]).unwrap();

        let error = read_file(&big).unwrap_err();

        assert!(error.message.starts_with("big.json: "), "{}", error.message);
        #[cfg(unix)]
        {
            let link = dir.path().join("link.json");
            std::os::unix::fs::symlink(&big, &link).unwrap();
            assert!(read_file(&link).is_err());
        }
    }

    #[test]
    fn display_names_are_cleaned_and_cut() {
        let long = "n".repeat(10 * 1024);
        assert_eq!(clean_display_name("  Get ", "U"), ("Get".into(), false));
        assert_eq!(clean_display_name("a\u{0}b\n", "U"), ("ab".into(), false));
        assert_eq!(
            clean_display_name("a\u{202E}b\u{200B}c\u{FEFF}\u{2066}d", "U").0,
            "abcd"
        );
        assert_eq!(
            clean_display_name("   ", "Untitled request").0,
            "Untitled request"
        );
        let (cut, was_cut) = clean_display_name(&long, "U");
        assert_eq!(cut.chars().count(), MAX_NAME_CHARS);
        assert!(was_cut);
    }
}
