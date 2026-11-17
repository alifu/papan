//! Response bodies on disk: `<app cache dir>/responses/<request_id>.bin` (plan 03 §6).
//! The directory is emptied at startup and only the most recent responses are kept.

use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use base64::Engine as _;

use super::types::{BodyEncoding, ResponseBody};
use crate::error::AppError;

pub const RESPONSES_DIR: &str = "responses";
pub const MAX_BODY_BYTES: u64 = 100 * 1024 * 1024;
pub const PREVIEW_TEXT_BYTES: usize = 2 * 1024 * 1024;
pub const PREVIEW_IMAGE_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_KEPT: usize = 20;
const BODY_SUFFIX: &str = ".bin";
const IMAGE_TYPES: [&str; 7] = [
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/bmp",
    "image/x-icon",
    "image/svg+xml",
];

/// The id when `text` is a lowercase hyphenated UUID: it names a file, so nothing else may
/// reach a path.
pub fn validate_id(text: &str) -> Result<String, AppError> {
    uuid::Uuid::parse_str(text)
        .ok()
        .map(|id| id.hyphenated().to_string())
        .filter(|canonical| canonical == text)
        .ok_or_else(|| {
            AppError::invalid_input("That request id is not valid.")
                .with_detail(format!("{text:?}"))
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Kept {
    id: String,
    content_type: Option<String>,
}

#[derive(Debug)]
pub struct Responses {
    dir: PathBuf,
    /// Newest first.
    kept: Mutex<VecDeque<Kept>>,
}

impl Responses {
    /// Uses `<cache_dir>/responses`, emptied first.
    pub fn new(cache_dir: &Path) -> Result<Self, AppError> {
        let dir = cache_dir.join(RESPONSES_DIR);
        match std::fs::remove_dir_all(&dir) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            kept: Mutex::new(VecDeque::new()),
        })
    }

    fn lock(&self) -> MutexGuard<'_, VecDeque<Kept>> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Where the body of `id` is written. `id` must come from [`validate_id`].
    pub fn path(&self, id: &str) -> Result<PathBuf, AppError> {
        Ok(self.dir.join(format!("{}{BODY_SUFFIX}", validate_id(id)?)))
    }

    /// Records a finished response and deletes the files beyond [`MAX_KEPT`].
    pub fn keep(&self, id: &str, content_type: Option<String>) {
        let evicted: Vec<Kept> = {
            let mut kept = self.lock();
            kept.retain(|entry| entry.id != id);
            kept.push_front(Kept {
                id: id.to_string(),
                content_type,
            });
            let excess = kept.len().saturating_sub(MAX_KEPT);
            (0..excess).filter_map(|_| kept.pop_back()).collect()
        };
        for entry in evicted {
            if let Ok(path) = self.path(&entry.id) {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    fn find(&self, id: &str) -> Result<Kept, AppError> {
        validate_id(id)?;
        self.lock()
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
            .ok_or_else(|| AppError::not_found("That response is no longer available."))
    }

    /// Whether a response is already stored under `id`.
    pub fn is_kept(&self, id: &str) -> bool {
        self.lock().iter().any(|entry| entry.id == id)
    }

    /// The body file of a kept response.
    pub fn file_of(&self, id: &str) -> Result<PathBuf, AppError> {
        self.find(id)?;
        self.path(id)
    }

    pub fn preview(&self, id: &str) -> Result<ResponseBody, AppError> {
        let kept = self.find(id)?;
        let path = self.path(id)?;
        let content_type = kept.content_type.as_deref().unwrap_or_default();
        let mime: Option<mime::Mime> = content_type.parse().ok();
        if is_image(mime.as_ref()) {
            let (bytes, truncated) = read_capped(&path, PREVIEW_IMAGE_BYTES)?;
            let data = base64::engine::general_purpose::STANDARD.encode(bytes);
            return Ok(body(BodyEncoding::Base64, data, truncated));
        }
        let (bytes, truncated) = read_capped(&path, PREVIEW_TEXT_BYTES)?;
        if is_textual(mime.as_ref()) {
            let encoding = mime
                .as_ref()
                .and_then(|mime| mime.get_param(mime::CHARSET))
                .and_then(|charset| encoding_rs::Encoding::for_label(charset.as_str().as_bytes()))
                .unwrap_or(encoding_rs::UTF_8);
            let (text, _) = encoding.decode_without_bom_handling(&bytes);
            return Ok(body(BodyEncoding::Text, text.into_owned(), truncated));
        }
        Ok(match clean_utf8(&bytes) {
            Some(text) => body(BodyEncoding::Text, text, truncated),
            None => body(BodyEncoding::None, String::new(), false),
        })
    }
}

fn body(encoding: BodyEncoding, data: String, truncated: bool) -> ResponseBody {
    ResponseBody {
        encoding,
        data,
        truncated,
    }
}

fn read_capped(path: &Path, cap: usize) -> Result<(Vec<u8>, bool), AppError> {
    let file = std::fs::File::open(path)?;
    let limit = u64::try_from(cap).unwrap_or(u64::MAX).saturating_add(1);
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes)?;
    let truncated = bytes.len() > cap;
    bytes.truncate(cap);
    Ok((bytes, truncated))
}

fn is_image(mime: Option<&mime::Mime>) -> bool {
    mime.is_some_and(|mime| IMAGE_TYPES.contains(&mime.essence_str()))
}

fn is_textual(mime: Option<&mime::Mime>) -> bool {
    let Some(mime) = mime else {
        return false;
    };
    let subtype = mime.subtype().as_str();
    let suffix = mime.suffix().map(|name| name.as_str());
    mime.type_() == mime::TEXT
        || ["json", "xml", "javascript", "x-www-form-urlencoded"].contains(&subtype)
        || matches!(suffix, Some("json" | "xml"))
}

/// UTF-8 text without NUL bytes; a sequence cut off by the preview cap is allowed.
fn clean_utf8(bytes: &[u8]) -> Option<String> {
    let valid = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) if error.error_len().is_none() => {
            std::str::from_utf8(&bytes[..error.valid_up_to()]).ok()?
        }
        Err(_) => return None,
    };
    (!valid.contains('\0')).then(|| valid.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    fn id(n: u8) -> String {
        uuid::Uuid::from_bytes([n; 16]).hyphenated().to_string()
    }

    fn stored(responses: &Responses, n: u8, bytes: &[u8], content_type: Option<&str>) -> String {
        let id = id(n);
        std::fs::write(responses.path(&id).unwrap(), bytes).unwrap();
        responses.keep(&id, content_type.map(str::to_string));
        id
    }

    #[test]
    fn ids_that_are_not_canonical_uuids_are_rejected() {
        let upper = id(0xab).to_uppercase();
        for text in ["", "../x", "/etc/passwd", upper.as_str(), "x.bin"] {
            assert_eq!(validate_id(text).unwrap_err().code, ErrorCode::InvalidInput);
        }
        assert!(validate_id(&id(1)).is_ok());
    }

    #[test]
    fn startup_empties_the_directory() {
        let cache = tempfile::tempdir().unwrap();
        let dir = cache.path().join(RESPONSES_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("old.bin"), b"x").unwrap();

        Responses::new(cache.path()).unwrap();

        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    }

    #[test]
    fn only_the_most_recent_responses_are_kept() {
        let cache = tempfile::tempdir().unwrap();
        let responses = Responses::new(cache.path()).unwrap();

        let ids: Vec<String> = (0..=MAX_KEPT as u8)
            .map(|n| stored(&responses, n, b"x", None))
            .collect();

        assert_eq!(
            responses.preview(&ids[0]).unwrap_err().code,
            ErrorCode::NotFound
        );
        assert!(!responses.path(&ids[0]).unwrap().exists());
        assert!(responses.file_of(&ids[MAX_KEPT]).unwrap().exists());
        assert_eq!(
            std::fs::read_dir(cache.path().join(RESPONSES_DIR))
                .unwrap()
                .count(),
            MAX_KEPT
        );
    }

    #[test]
    fn previews_follow_the_content_type() {
        let cache = tempfile::tempdir().unwrap();
        let responses = Responses::new(cache.path()).unwrap();
        let big = "a".repeat(PREVIEW_TEXT_BYTES + 1);
        let cases = [
            (
                &b"{\"a\":1}"[..],
                Some("application/problem+json"),
                BodyEncoding::Text,
                "{\"a\":1}",
                false,
            ),
            (
                &b"caf\xe9"[..],
                Some("text/plain; charset=iso-8859-1"),
                BodyEncoding::Text,
                "café",
                false,
            ),
            (
                &b"\x89PNG"[..],
                Some("image/png"),
                BodyEncoding::Base64,
                "iVBORw==",
                false,
            ),
            (&b"plain"[..], None, BodyEncoding::Text, "plain", false),
            (
                &b"\xff\x00"[..],
                Some("application/octet-stream"),
                BodyEncoding::None,
                "",
                false,
            ),
            (&b"a\0b"[..], None, BodyEncoding::None, "", false),
        ];
        for (n, (bytes, content_type, encoding, data, truncated)) in cases.into_iter().enumerate() {
            let id = stored(&responses, n as u8, bytes, content_type);
            let preview = responses.preview(&id).unwrap();
            assert_eq!(
                preview,
                body(encoding, data.to_string(), truncated),
                "{content_type:?}"
            );
        }
        let id = stored(&responses, 99, big.as_bytes(), Some("text/plain"));
        let preview = responses.preview(&id).unwrap();
        assert!(preview.truncated);
        assert_eq!(preview.data.len(), PREVIEW_TEXT_BYTES);
    }

    #[test]
    fn a_utf8_sequence_cut_by_the_cap_still_reads_as_text() {
        assert_eq!(
            clean_utf8("é".as_bytes()[..1].as_ref()),
            Some(String::new())
        );
        assert_eq!(clean_utf8(b"\xff"), None);
    }
}
