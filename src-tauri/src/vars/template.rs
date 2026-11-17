//! The `{{name}}` lexer (plan 04 §5 grammar). The frontend tokenizer implements the same
//! grammar; both run `tests/fixtures/vars/template-cases.json`.

use std::ops::Range;

/// Longest variable name, in characters.
pub const MAX_NAME_CHARS: usize = 128;

const OPEN: &str = "{{";
const ESCAPED_OPEN: &str = "\\{{";

/// A piece of a template. `range` is in bytes and covers the whole token with its braces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment<'a> {
    Literal(&'a str),
    Var {
        name: &'a str,
        range: Range<usize>,
    },
    /// `\{{`: stands for a literal `{{`.
    Escape,
}

/// Splits `text` into literals, variable tokens and escapes. Never fails: anything that
/// is not a well-formed token is literal text.
pub fn tokens(text: &str) -> Vec<Segment<'_>> {
    let mut segments = Vec::new();
    let mut literal_start = 0;
    let mut index = 0;
    while index < text.len() {
        let rest = &text[index..];
        let found = if rest.starts_with(ESCAPED_OPEN) {
            Some((Segment::Escape, index + ESCAPED_OPEN.len()))
        } else if rest.starts_with(OPEN) {
            var_at(text, index).map(|(name, end)| {
                let range = index..end;
                (Segment::Var { name, range }, end)
            })
        } else {
            None
        };
        match found {
            Some((segment, end)) => {
                if literal_start < index {
                    segments.push(Segment::Literal(&text[literal_start..index]));
                }
                segments.push(segment);
                index = end;
                literal_start = end;
            }
            None => index += rest.chars().next().map_or(1, char::len_utf8),
        }
    }
    if literal_start < text.len() {
        segments.push(Segment::Literal(&text[literal_start..]));
    }
    segments
}

/// Names of the variable tokens in `text`, in order of appearance.
pub fn names(text: &str) -> Vec<&str> {
    tokens(text)
        .into_iter()
        .filter_map(|segment| match segment {
            Segment::Var { name, .. } => Some(name),
            _ => None,
        })
        .collect()
}

/// Whether `text` is exactly one variable token and nothing else.
pub fn is_single_reference(text: &str) -> bool {
    matches!(tokens(text).as_slice(), [Segment::Var { .. }])
}

/// The token starting at `start` (which holds `{{`): its trimmed name and end offset.
/// The scan stops at the first brace or line break, so a failed attempt is cheap.
fn var_at(text: &str, start: usize) -> Option<(&str, usize)> {
    let inner_start = start + OPEN.len();
    let inner_len = text[inner_start..].find(['{', '}', '\r', '\n'])?;
    let close = inner_start + inner_len;
    if !text[close..].starts_with("}}") {
        return None;
    }
    let name = text[inner_start..close].trim_matches(' ');
    let length = name.chars().count();
    (1..=MAX_NAME_CHARS)
        .contains(&length)
        .then_some((name, close + 2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct Case {
        text: String,
        tokens: Vec<Expected>,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Expected {
        from: usize,
        to: usize,
        name: String,
    }

    fn utf16_offset(text: &str, byte: usize) -> usize {
        text[..byte].encode_utf16().count()
    }

    #[test]
    fn shared_template_cases_pass() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/vars/template-cases.json"
        );
        let cases: Vec<Case> =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(cases.len() >= 15, "fixtures missing");

        for case in cases {
            let found: Vec<Expected> = tokens(&case.text)
                .into_iter()
                .filter_map(|segment| match segment {
                    Segment::Var { name, range } => Some(Expected {
                        from: utf16_offset(&case.text, range.start),
                        to: utf16_offset(&case.text, range.end),
                        name: name.to_string(),
                    }),
                    _ => None,
                })
                .collect();
            assert_eq!(found, case.tokens, "{:?}", case.text);
        }
    }

    #[test]
    fn literals_and_escapes_rebuild_the_text() {
        let segments = tokens("a \\{{b}} {{ c }}!");

        assert_eq!(
            segments,
            [
                Segment::Literal("a "),
                Segment::Escape,
                Segment::Literal("b}} "),
                Segment::Var {
                    name: "c",
                    range: 9..16
                },
                Segment::Literal("!"),
            ]
        );
    }

    #[test]
    fn single_reference_and_names() {
        assert!(is_single_reference("{{token}}"));
        assert!(!is_single_reference(" {{token}}"));
        assert!(!is_single_reference("{{a}}{{b}}"));
        assert!(!is_single_reference("\\{{t}}"));
        assert_eq!(names("{{a}} x {{ b }}"), ["a", "b"]);
    }
}
