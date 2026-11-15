//! Path matching and the in-memory capture ring (plan 07 §6, D4). Captures are never
//! written to disk: incoming headers and bodies often hold tokens and signatures.

use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use http::request::Parts;

use super::types::{Capture, CaptureSummary};
use crate::store::types::KeyValue;

/// Bodies held per listener, on top of the per-capture cap.
const MAX_RING_BYTES: usize = 64 * 1024 * 1024;

/// `/hooks` matches `/hooks` and `/hooks/x`, not `/hooksx`.
pub fn path_matches(configured: &str, request_path: &str) -> bool {
    let base = configured.trim_end_matches('/');
    match request_path.strip_prefix(base) {
        Some(rest) => rest.is_empty() || rest.starts_with('/'),
        None => false,
    }
}

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |elapsed| elapsed.as_millis() as f64)
}

fn header_pairs(parts: &Parts) -> Vec<KeyValue> {
    parts
        .headers
        .iter()
        .map(|(name, value)| KeyValue {
            name: name.as_str().to_string(),
            value: String::from_utf8_lossy(value.as_bytes()).into_owned(),
            enabled: true,
        })
        .collect()
}

/// A capture of `parts` and `body`; `body` is `None` when it was over the cap, and
/// `declared_size` then is what the client announced (0 when it did not).
pub fn build_capture(
    parts: &Parts,
    remote: &str,
    body: Option<&[u8]>,
    declared_size: usize,
) -> Capture {
    let content_type = parts
        .headers
        .get(http::header::CONTENT_TYPE)
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
        .unwrap_or_default();
    let bytes = body.unwrap_or_default();
    let (body_text, body_base64) = match std::str::from_utf8(bytes) {
        _ if bytes.is_empty() => (None, None),
        Ok(text) => (Some(text.to_string()), None),
        Err(_) => (
            None,
            Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
        ),
    };
    let size = if body.is_some() {
        bytes.len()
    } else {
        declared_size
    };
    Capture {
        summary: CaptureSummary {
            id: uuid::Uuid::new_v4().to_string(),
            at_ms: now_ms(),
            method: parts.method.as_str().to_string(),
            path: parts.uri.path().to_string(),
            size: u32::try_from(size).unwrap_or(u32::MAX),
            content_type,
        },
        query: parts.uri.query().unwrap_or_default().to_string(),
        headers: header_pairs(parts),
        remote: remote.to_string(),
        body_text,
        body_base64,
        truncated: body.is_none(),
    }
}

fn held_bytes(capture: &Capture) -> usize {
    capture.body_text.as_ref().map_or(0, String::len)
        + capture.body_base64.as_ref().map_or(0, String::len)
}

/// Newest captures, bounded by count and by total body bytes.
#[derive(Debug)]
pub struct CaptureRing {
    items: VecDeque<Capture>,
    bytes: usize,
    max_items: usize,
    max_bytes: usize,
}

impl CaptureRing {
    pub fn new(max_items: usize) -> Self {
        Self::with_byte_cap(max_items, MAX_RING_BYTES)
    }

    fn with_byte_cap(max_items: usize, max_bytes: usize) -> Self {
        Self {
            items: VecDeque::new(),
            bytes: 0,
            max_items,
            max_bytes,
        }
    }

    pub fn push(&mut self, capture: Capture) {
        self.bytes += held_bytes(&capture);
        self.items.push_back(capture);
        while self.items.len() > self.max_items
            || (self.bytes > self.max_bytes && self.items.len() > 1)
        {
            if let Some(oldest) = self.items.pop_front() {
                self.bytes -= held_bytes(&oldest);
            }
        }
    }

    /// Applies a changed `max_captures`, dropping the oldest that no longer fit.
    pub fn set_max_items(&mut self, max_items: usize) {
        self.max_items = max_items;
        while self.items.len() > self.max_items {
            if let Some(oldest) = self.items.pop_front() {
                self.bytes -= held_bytes(&oldest);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Newest first.
    pub fn summaries(&self) -> Vec<CaptureSummary> {
        self.items.iter().rev().map(|c| c.summary.clone()).collect()
    }

    pub fn find(&self, capture_id: &str) -> Option<Capture> {
        self.items
            .iter()
            .find(|c| c.summary.id == capture_id)
            .cloned()
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(method: &str, uri: &str, headers: &[(&str, &str)]) -> Parts {
        let mut builder = http::Request::builder().method(method).uri(uri);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        builder.body(()).unwrap().into_parts().0
    }

    fn capture(body: &[u8]) -> Capture {
        build_capture(&parts("POST", "/h", &[]), "127.0.0.1:1", Some(body), 0)
    }

    #[test]
    fn path_matching_respects_segments() {
        assert!(path_matches("/hooks", "/hooks"));
        assert!(path_matches("/hooks", "/hooks/x"));
        assert!(path_matches("/hooks/", "/hooks/x"));
        assert!(!path_matches("/hooks", "/hooksx"));
        assert!(!path_matches("/hooks", "/other"));
        assert!(path_matches("/", "/anything"));
    }

    #[test]
    fn utf8_body_is_text_and_binary_is_base64() {
        let text = capture(b"{\"a\":1}");
        assert_eq!(text.body_text.as_deref(), Some("{\"a\":1}"));
        assert!(text.body_base64.is_none());
        let binary = capture(&[0xff, 0x00, 0xfe]);
        assert!(binary.body_text.is_none());
        assert_eq!(binary.body_base64.as_deref(), Some("/wD+"));
        assert_eq!(binary.summary.size, 3);
        let empty = capture(b"");
        assert!(empty.body_text.is_none() && empty.body_base64.is_none());
    }

    #[test]
    fn capture_records_request_fields() {
        let p = parts(
            "PUT",
            "/hooks/x?a=1&b=2",
            &[("Content-Type", "text/plain"), ("X-Sig", "s")],
        );
        let c = build_capture(&p, "10.0.0.2:5555", Some(b"hi"), 0);
        assert_eq!(c.summary.method, "PUT");
        assert_eq!(c.summary.path, "/hooks/x");
        assert_eq!(c.query, "a=1&b=2");
        assert_eq!(c.summary.content_type, "text/plain");
        assert_eq!(c.remote, "10.0.0.2:5555");
        assert!(c
            .headers
            .iter()
            .any(|h| h.name == "x-sig" && h.value == "s"));
        assert!(!c.truncated);
    }

    #[test]
    fn oversized_body_is_truncated_with_declared_size() {
        let c = build_capture(&parts("POST", "/h", &[]), "r", None, 5000);
        assert!(c.truncated);
        assert!(c.body_text.is_none() && c.body_base64.is_none());
        assert_eq!(c.summary.size, 5000);
    }

    #[test]
    fn ring_evicts_oldest_by_count() {
        let mut ring = CaptureRing::new(2);
        let ids: Vec<String> = (0..3)
            .map(|i| {
                let c = capture(format!("b{i}").as_bytes());
                let id = c.summary.id.clone();
                ring.push(c);
                id
            })
            .collect();
        assert_eq!(ring.len(), 2);
        assert!(ring.find(&ids[0]).is_none());
        let listed: Vec<String> = ring.summaries().into_iter().map(|s| s.id).collect();
        assert_eq!(listed, vec![ids[2].clone(), ids[1].clone()]);
    }

    #[test]
    fn ring_evicts_by_bytes_but_keeps_the_newest() {
        let mut ring = CaptureRing::with_byte_cap(100, 10);
        ring.push(capture(b"123456"));
        ring.push(capture(b"abcdefgh"));
        assert_eq!(ring.len(), 1);
        ring.push(capture(&[b'x'; 50]));
        assert_eq!(ring.len(), 1);
        ring.clear();
        assert!(ring.is_empty());
    }
}
