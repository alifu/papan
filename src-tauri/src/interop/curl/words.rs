//! POSIX word splitting for a pasted command (plan 03b §7). Nothing is expanded or
//! executed: `$VAR`, backticks and `$(…)` stay literal text.

use std::iter::Peekable;
use std::str::Chars;

/// Why a command could not be split into words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitError {
    UnterminatedQuote,
}

/// Splits `text` into shell words. Total: any input gives words or an error, never a panic.
pub fn split(text: &str) -> Result<Vec<String>, SplitError> {
    let mut chars = text.chars().peekable();
    let mut words = Vec::new();
    let mut current: Option<String> = None;
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\n' | '\r' => words.extend(current.take()),
            '\\' => match chars.next() {
                Some('\n') => {}
                Some('\r') if chars.peek() == Some(&'\n') => {
                    chars.next();
                }
                Some(next) => current.get_or_insert_with(String::new).push(next),
                None => current.get_or_insert_with(String::new).push('\\'),
            },
            '\'' => single_quoted(&mut chars, current.get_or_insert_with(String::new))?,
            '"' => double_quoted(&mut chars, current.get_or_insert_with(String::new))?,
            '$' if chars.peek() == Some(&'\'') => {
                chars.next();
                ansi_c_quoted(&mut chars, current.get_or_insert_with(String::new))?;
            }
            other => current.get_or_insert_with(String::new).push(other),
        }
    }
    words.extend(current);
    Ok(words)
}

fn single_quoted(chars: &mut Peekable<Chars<'_>>, out: &mut String) -> Result<(), SplitError> {
    for c in chars.by_ref() {
        if c == '\'' {
            return Ok(());
        }
        out.push(c);
    }
    Err(SplitError::UnterminatedQuote)
}

fn double_quoted(chars: &mut Peekable<Chars<'_>>, out: &mut String) -> Result<(), SplitError> {
    while let Some(c) = chars.next() {
        match c {
            '"' => return Ok(()),
            '\\' => match chars.next() {
                Some('\n') => {}
                Some('\r') if chars.peek() == Some(&'\n') => {
                    chars.next();
                }
                Some(next @ ('$' | '`' | '"' | '\\')) => out.push(next),
                Some(next) => {
                    out.push('\\');
                    out.push(next);
                }
                None => break,
            },
            other => out.push(other),
        }
    }
    Err(SplitError::UnterminatedQuote)
}

fn ansi_c_quoted(chars: &mut Peekable<Chars<'_>>, out: &mut String) -> Result<(), SplitError> {
    while let Some(c) = chars.next() {
        match c {
            '\'' => return Ok(()),
            '\\' => match chars.next() {
                Some(next) => ansi_escape(next, chars, out),
                None => break,
            },
            other => out.push(other),
        }
    }
    Err(SplitError::UnterminatedQuote)
}

fn ansi_escape(c: char, chars: &mut Peekable<Chars<'_>>, out: &mut String) {
    let simple = match c {
        'n' => Some('\n'),
        't' => Some('\t'),
        'r' => Some('\r'),
        'a' => Some('\u{7}'),
        'b' => Some('\u{8}'),
        'e' | 'E' => Some('\u{1b}'),
        'f' => Some('\u{c}'),
        'v' => Some('\u{b}'),
        '\\' | '\'' | '"' | '?' => Some(c),
        _ => None,
    };
    if let Some(simple) = simple {
        out.push(simple);
        return;
    }
    let code = match c {
        'x' => digits(chars, 16, 2),
        'u' => digits(chars, 16, 4),
        'U' => digits(chars, 16, 8),
        '0'..='7' => {
            let first = c.to_digit(8).unwrap_or(0);
            Some(more_digits(chars, 8, 2, first))
        }
        _ => None,
    };
    match code.and_then(char::from_u32) {
        Some(decoded) => out.push(decoded),
        None => {
            out.push('\\');
            out.push(c);
        }
    }
}

/// Reads up to `max` digits in `radix`; `None` when there is none.
fn digits(chars: &mut Peekable<Chars<'_>>, radix: u32, max: usize) -> Option<u32> {
    let first = chars.peek().and_then(|c| c.to_digit(radix))?;
    chars.next();
    Some(more_digits(chars, radix, max - 1, first))
}

/// Continues `value` with up to `max` further digits in `radix`.
fn more_digits(chars: &mut Peekable<Chars<'_>>, radix: u32, max: usize, value: u32) -> u32 {
    let mut value = value;
    for _ in 0..max {
        let Some(digit) = chars.peek().and_then(|c| c.to_digit(radix)) else {
            break;
        };
        chars.next();
        value = value.saturating_mul(radix).saturating_add(digit);
    }
    value
}

#[cfg(test)]
#[path = "words_tests.rs"]
mod tests;
