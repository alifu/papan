//! The bounded in-memory log of one connection (plan 07 §5): at most 1000 entries and
//! 32 MiB of text, the oldest dropped first; one payload keeps at most 1 MiB of text.
//! Snapshots are immutable copies.

use std::collections::VecDeque;

use super::types::{WsDirection, WsFrame, WsMessage};

pub const MAX_ENTRIES: usize = 1000;
pub const MAX_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub entries: usize,
    pub bytes: usize,
    pub payload: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            entries: MAX_ENTRIES,
            bytes: MAX_BYTES,
            payload: MAX_PAYLOAD_BYTES,
        }
    }
}

/// What to record: `text` is what is shown (masked text, or base64 for binary) and
/// `size` the payload size on the wire.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub at_ms: f64,
    pub direction: WsDirection,
    pub frame: WsFrame,
    pub text: String,
    pub size: usize,
}

#[derive(Debug)]
pub struct WsLog {
    limits: Limits,
    entries: VecDeque<WsMessage>,
    bytes: usize,
    next_seq: u32,
}

impl Default for WsLog {
    fn default() -> Self {
        Self::with_limits(Limits::default())
    }
}

/// `text` cut to at most `max` bytes on a character boundary.
fn cut(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let end = (0..=max)
        .rev()
        .find(|&index| text.is_char_boundary(index))
        .unwrap_or(0);
    &text[..end]
}

impl WsLog {
    pub fn with_limits(limits: Limits) -> Self {
        Self {
            limits,
            entries: VecDeque::new(),
            bytes: 0,
            next_seq: 1,
        }
    }

    /// Records `entry` and returns it as logged (sequence number, truncation).
    pub fn push(&mut self, entry: Entry) -> WsMessage {
        let kept = cut(&entry.text, self.limits.payload);
        let message = WsMessage {
            seq: self.next_seq,
            at_ms: entry.at_ms,
            direction: entry.direction,
            frame: entry.frame,
            truncated: kept.len() < entry.text.len(),
            text: kept.to_string(),
            size: u32::try_from(entry.size).unwrap_or(u32::MAX),
        };
        self.next_seq = self.next_seq.wrapping_add(1);
        self.bytes += message.text.len();
        self.entries.push_back(message.clone());
        self.evict();
        message
    }

    fn evict(&mut self) {
        while self.entries.len() > self.limits.entries || self.bytes > self.limits.bytes {
            let Some(oldest) = self.entries.pop_front() else {
                return;
            };
            self.bytes -= oldest.text.len();
        }
    }

    pub fn snapshot(&self) -> Vec<WsMessage> {
        self.entries.iter().cloned().collect()
    }

    /// Empties the log; sequence numbers keep counting.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(text: &str) -> Entry {
        Entry {
            at_ms: 1.0,
            direction: WsDirection::Received,
            frame: WsFrame::Text,
            text: text.to_string(),
            size: text.len(),
        }
    }

    #[test]
    fn sequence_numbers_grow_and_survive_a_clear() {
        let mut log = WsLog::default();

        let first = log.push(entry("a"));
        let second = log.push(entry("b"));
        log.clear();
        let third = log.push(entry("c"));

        assert_eq!((first.seq, second.seq, third.seq), (1, 2, 3));
        assert_eq!(log.snapshot().len(), 1);
    }

    #[test]
    fn the_oldest_entry_goes_when_the_count_cap_is_exceeded() {
        let mut log = WsLog::with_limits(Limits {
            entries: 3,
            ..Limits::default()
        });

        (0..4).for_each(|n| {
            log.push(entry(&n.to_string()));
        });

        let texts: Vec<String> = log.snapshot().into_iter().map(|m| m.text).collect();
        assert_eq!(texts, ["1", "2", "3"]);
    }

    #[test]
    fn the_byte_cap_drops_the_oldest_until_it_fits() {
        let mut log = WsLog::with_limits(Limits {
            bytes: 10,
            ..Limits::default()
        });

        ["aaaa", "bbbb", "cccc"].iter().for_each(|t| {
            log.push(entry(t));
        });

        let texts: Vec<String> = log.snapshot().into_iter().map(|m| m.text).collect();
        assert_eq!(texts, ["bbbb", "cccc"]);
    }

    #[test]
    fn a_long_payload_is_cut_on_a_char_boundary_and_flagged() {
        let mut log = WsLog::with_limits(Limits {
            payload: 5,
            ..Limits::default()
        });

        let kept = log.push(entry("ab\u{e9}\u{e9}\u{e9}"));

        assert_eq!(kept.text, "ab\u{e9}");
        assert!(kept.truncated);
        assert_eq!(kept.size, 8);
        assert!(!log.push(entry("short")).truncated);
    }

    #[test]
    fn a_snapshot_is_not_changed_by_later_pushes() {
        let mut log = WsLog::default();
        log.push(entry("a"));

        let snapshot = log.snapshot();
        log.push(entry("b"));

        assert_eq!(snapshot.len(), 1);
    }
}
