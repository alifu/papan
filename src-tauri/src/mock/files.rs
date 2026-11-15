//! File bodies for mock responses (plan 11 §9): a response can send a file from inside the
//! collection. The path is checked at save time and again here after links are resolved.

use std::io::Read as _;
use std::path::Path;

use super::model::check_file;

pub const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

/// Reads `relative` below `root`; the message is safe to show (no absolute paths).
pub fn read(root: &Path, relative: &str) -> Result<Vec<u8>, String> {
    check_file("mock", "file", relative).map_err(|e| e.message)?;
    let base = root
        .canonicalize()
        .map_err(|_| "The collection folder is not readable.".to_string())?;
    let target = base
        .join(relative)
        .canonicalize()
        .map_err(|_| format!("File \"{relative}\" was not found in the collection."))?;
    // The resolved path decides, not the requested text: a link inside the collection must
    // not lead into a hidden folder such as `.git`.
    let inside = target
        .strip_prefix(&base)
        .map_err(|_| format!("File \"{relative}\" is outside the collection."))?;
    if inside
        .components()
        .any(|part| part.as_os_str().to_string_lossy().starts_with('.'))
    {
        return Err(format!("File \"{relative}\" is not available."));
    }
    // One open handle for the checks and the read, so the file cannot change in between.
    let mut file =
        std::fs::File::open(&target).map_err(|_| "The file is not readable.".to_string())?;
    let meta = file
        .metadata()
        .map_err(|_| "The file is not readable.".to_string())?;
    if !meta.is_file() {
        return Err(format!("\"{relative}\" is not a file."));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(format!("File \"{relative}\" is over 10 MiB."));
    }
    let mut bytes = Vec::new();
    std::io::Read::take(&mut file, MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "The file is not readable.".to_string())?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(format!("File \"{relative}\" is over 10 MiB."));
    }
    Ok(bytes)
}

/// A content type from the extension; unknown files are sent as bytes.
pub fn content_type_of(relative: &str) -> &'static str {
    let extension = relative
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "json" => "application/json",
        "html" | "htm" => "text/html; charset=utf-8",
        "txt" | "md" => "text/plain; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "xml" => "application/xml",
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collection() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("files")).unwrap();
        std::fs::write(dir.path().join("files/a.json"), b"{\"a\":1}").unwrap();
        dir
    }

    #[test]
    fn reads_a_file_inside_the_collection() {
        let dir = collection();
        assert_eq!(read(dir.path(), "files/a.json").unwrap(), b"{\"a\":1}");
    }

    #[test]
    fn refuses_escapes_and_missing_files_without_leaking_paths() {
        let dir = collection();
        std::fs::write(dir.path().join(".secret"), b"x").unwrap();
        for bad in ["../x", "/etc/passwd", ".secret", "files", "nope.txt"] {
            let message = read(dir.path(), bad).unwrap_err();
            assert!(
                !message.contains(dir.path().to_str().unwrap()),
                "{bad}: {message}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_link_that_points_outside() {
        let dir = collection();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("s.txt"), b"secret").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("files/out")).unwrap();
        assert!(read(dir.path(), "files/out/s.txt").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_link_that_leads_into_a_hidden_folder() {
        let dir = collection();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".git/config"), b"url = x").unwrap();
        std::os::unix::fs::symlink(dir.path().join(".git"), dir.path().join("pub")).unwrap();
        assert!(read(dir.path(), "pub/config").is_err());
    }

    #[test]
    fn guesses_content_types() {
        assert_eq!(content_type_of("a/b.JSON"), "application/json");
        assert_eq!(content_type_of("x.bin"), "application/octet-stream");
    }
}
