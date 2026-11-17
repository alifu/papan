//! Masks secret values in text we show or log (URLs, error text). A URL may carry a
//! secret percent-encoded (any encode set, either hex case), with `+` for a space, or
//! with its host lowercased, so matching runs on the percent-decoded bytes and ignores
//! ASCII case. Over-masking is harmless; a miss is not.

use crate::logging::redact::REDACTED;

/// One decoded byte and the span of `text` it came from.
struct Decoded {
    bytes: Vec<u8>,
    spans: Vec<(usize, usize)>,
}

fn hex(byte: u8) -> Option<u8> {
    char::from(byte)
        .to_digit(16)
        .and_then(|d| u8::try_from(d).ok())
}

fn decode(text: &str, plus_is_space: bool) -> Decoded {
    let raw = text.as_bytes();
    let mut decoded = Decoded {
        bytes: Vec::with_capacity(raw.len()),
        spans: Vec::with_capacity(raw.len()),
    };
    let mut index = 0;
    while index < raw.len() {
        let escaped = (raw[index] == b'%')
            .then(|| Some(hex(*raw.get(index + 1)?)? * 16 + hex(*raw.get(index + 2)?)?))
            .flatten();
        let (byte, width) = match (escaped, raw[index]) {
            (Some(byte), _) => (byte, 3),
            (None, b'+') if plus_is_space => (b' ', 1),
            (None, byte) => (byte, 1),
        };
        decoded.bytes.push(byte);
        decoded.spans.push((index, index + width));
        index += width;
    }
    decoded
}

/// Marks every decoded byte that belongs to an occurrence of a secret.
fn covered(decoded: &[u8], secrets: &[&str]) -> Vec<bool> {
    let mut marks = vec![false; decoded.len()];
    for secret in secrets
        .iter()
        .map(|s| s.as_bytes())
        .filter(|s| !s.is_empty())
    {
        if secret.len() > decoded.len() {
            continue;
        }
        for start in 0..=decoded.len() - secret.len() {
            if decoded[start..start + secret.len()].eq_ignore_ascii_case(secret) {
                marks[start..start + secret.len()].fill(true);
            }
        }
    }
    marks
}

fn mask_pass(text: &str, secrets: &[&str], plus_is_space: bool) -> String {
    let decoded = decode(text, plus_is_space);
    let marks = covered(&decoded.bytes, secrets);
    if !marks.contains(&true) {
        return text.to_string();
    }
    let mut out = Vec::with_capacity(text.len());
    let mut in_secret = false;
    for (&(start, end), &mark) in decoded.spans.iter().zip(&marks) {
        if mark && !in_secret {
            out.extend_from_slice(REDACTED.as_bytes());
        }
        if !mark {
            out.extend_from_slice(&text.as_bytes()[start..end]);
        }
        in_secret = mark;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `text` with every occurrence of each secret, in any encoding above, masked.
pub fn mask_secrets(text: &str, secrets: &[&str]) -> String {
    let literal_plus = mask_pass(text, secrets, false);
    mask_pass(&literal_plus, secrets, true)
}
