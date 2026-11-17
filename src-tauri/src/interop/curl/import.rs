//! `curl …` text to a request (plan 03b §7). Pure: nothing is run, no file is read and
//! no environment variable is looked at. A `@file` argument keeps its path, which is not
//! granted, so Send is refused until the user picks the file.

use super::build;
use super::options::{self, Opt, Spec};
use super::words::{self, SplitError};
use crate::engine::http::types::CurlImport;
use crate::error::AppError;
use crate::store::types::{HttpVersion, KeyValue, Part};

/// Longest command text accepted.
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;
const MS_PER_SECOND: f64 = 1000.0;

/// One `-d`-style argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Data {
    Text(String),
    File(String),
}

/// Everything read from the options, before it becomes a request.
#[derive(Debug, Default)]
pub(super) struct Parsed {
    pub(super) method: Option<String>,
    pub(super) urls: Vec<String>,
    pub(super) headers: Vec<KeyValue>,
    pub(super) data: Vec<Data>,
    pub(super) json: Vec<String>,
    pub(super) parts: Vec<Part>,
    pub(super) user: Option<String>,
    pub(super) digest: bool,
    pub(super) cookies: Vec<String>,
    pub(super) get: bool,
    pub(super) head: bool,
    pub(super) follow_redirects: bool,
    pub(super) insecure: bool,
    pub(super) timeout_ms: Option<u32>,
    pub(super) http_version: HttpVersion,
    /// Names given as `-H 'Name:'`, which cURL removes instead of sending.
    pub(super) removed_headers: Vec<String>,
    /// An unknown option was seen; its argument, if any, may have landed in `urls`.
    pub(super) saw_unknown: bool,
    pub(super) warnings: Vec<String>,
}

/// Parses a cURL command. Refused when it is too long, does not start with `curl`, has an
/// unterminated quote or names no URL.
pub fn import(text: &str) -> Result<CurlImport, AppError> {
    if text.len() > MAX_INPUT_BYTES {
        return Err(AppError::invalid_input(
            "The cURL command is longer than 1 MiB.",
        ));
    }
    let words = words::split(text).map_err(|SplitError::UnterminatedQuote| {
        AppError::invalid_input("The cURL command has an unterminated quote.")
    })?;
    let Some((first, rest)) = words.split_first() else {
        return Err(not_curl());
    };
    if first != "curl" && !first.ends_with("/curl") {
        return Err(not_curl());
    }
    let mut parsed = Parsed::default();
    parse_words(rest, &mut parsed);
    build::build(parsed)
}

fn not_curl() -> AppError {
    AppError::invalid_input("This is not a cURL command.")
}

fn parse_words(words: &[String], parsed: &mut Parsed) {
    let mut index = 0;
    let mut options_done = false;
    while index < words.len() {
        let word = &words[index];
        index += 1;
        if options_done || word == "-" || !word.starts_with('-') {
            parsed.urls.push(word.clone());
        } else if word == "--" {
            options_done = true;
        } else if let Some(name) = word.strip_prefix("--") {
            let next = words.get(index);
            if long_option(name, word, next, parsed) {
                index += 1;
            }
        } else if short_cluster(&word[1..], words.get(index), parsed) {
            index += 1;
        }
    }
}

/// Applies `--name`; says whether the next word was used as its argument.
fn long_option(name: &str, typed: &str, next: Option<&String>, parsed: &mut Parsed) -> bool {
    let Some(spec) = options::long(name) else {
        parsed.saw_unknown = true;
        unsupported(parsed, typed, "");
        return false;
    };
    with_argument(spec, typed, None, next, parsed)
}

/// Applies `-abc`; says whether the next word was used as an argument.
fn short_cluster(cluster: &str, next: Option<&String>, parsed: &mut Parsed) -> bool {
    for (offset, letter) in cluster.char_indices() {
        let typed = format!("-{letter}");
        let Some(spec) = options::short(letter) else {
            parsed.saw_unknown = true;
            unsupported(parsed, &typed, "");
            return false;
        };
        if spec.takes_arg {
            let attached = &cluster[offset + letter.len_utf8()..];
            let inline = (!attached.is_empty()).then_some(attached);
            return with_argument(spec, &typed, inline, next, parsed);
        }
        apply(spec.opt, &typed, "", parsed);
    }
    false
}

/// Applies an option, taking its argument from `inline` or else `next`; says whether
/// `next` was used.
fn with_argument(
    spec: Spec,
    typed: &str,
    inline: Option<&str>,
    next: Option<&String>,
    parsed: &mut Parsed,
) -> bool {
    if !spec.takes_arg {
        apply(spec.opt, typed, "", parsed);
        return false;
    }
    if let Some(value) = inline {
        apply(spec.opt, typed, value, parsed);
        return false;
    }
    match next {
        Some(value) => {
            apply(spec.opt, typed, value, parsed);
            true
        }
        None => {
            parsed.warnings.push(format!("missing value for {typed}"));
            false
        }
    }
}

fn unsupported(parsed: &mut Parsed, typed: &str, hint: &str) {
    parsed
        .warnings
        .push(format!("unsupported option: {typed}{hint}"));
}

fn apply(opt: Opt, typed: &str, value: &str, parsed: &mut Parsed) {
    match opt {
        Opt::Method => parsed.method = Some(value.to_string()),
        Opt::Url => parsed.urls.push(value.to_string()),
        Opt::Header => header(value, parsed),
        Opt::Data | Opt::DataBinary => parsed.data.push(data_of(value)),
        Opt::DataRaw => parsed.data.push(Data::Text(value.to_string())),
        Opt::DataUrlencode => urlencoded(value, parsed),
        Opt::Json if value.starts_with('@') => file_not_read(parsed, typed),
        Opt::Json => parsed.json.push(value.to_string()),
        Opt::Form => form(value, typed, parsed),
        Opt::FormString => form_string(value, typed, parsed),
        Opt::User => parsed.user = Some(value.to_string()),
        Opt::Digest => parsed.digest = true,
        Opt::UserAgent => push_header(parsed, "User-Agent", value),
        Opt::Referer => push_header(parsed, "Referer", value),
        Opt::Cookie if value.contains('=') => parsed.cookies.push(value.to_string()),
        Opt::Cookie => unsupported(
            parsed,
            &format!("{typed} <file>"),
            " (cookie files are not read)",
        ),
        Opt::Get => parsed.get = true,
        Opt::Head => parsed.head = true,
        Opt::Location => parsed.follow_redirects = true,
        Opt::Insecure => {
            parsed.insecure = true;
            parsed.warnings.push(format!(
                "{typed}: TLS certificate verification is turned off for this request"
            ));
        }
        Opt::MaxTime => max_time(value, typed, parsed),
        Opt::Http1 => parsed.http_version = HttpVersion::Http1,
        Opt::Http2 => parsed.http_version = HttpVersion::Http2,
        Opt::Ignored => {}
        Opt::Warned(hint) => unsupported(parsed, typed, hint),
    }
}

fn data_of(value: &str) -> Data {
    match value.strip_prefix('@') {
        Some(path) => Data::File(path.to_string()),
        None => Data::Text(value.to_string()),
    }
}

pub(super) fn file_not_read(parsed: &mut Parsed, typed: &str) {
    parsed.warnings.push(format!(
        "{typed} @file: the file is not read; add the body by hand"
    ));
}

fn push_header(parsed: &mut Parsed, name: &str, value: &str) {
    parsed.headers.push(KeyValue {
        name: name.to_string(),
        value: value.to_string(),
        enabled: true,
    });
}

fn header(line: &str, parsed: &mut Parsed) {
    if let Some((name, value)) = line.split_once(':') {
        if value.trim().is_empty() {
            parsed.removed_headers.push(name.trim().to_string());
        } else {
            push_header(parsed, name.trim(), value.trim_start());
        }
    } else if let Some(name) = line.strip_suffix(';') {
        push_header(parsed, name.trim(), "");
    } else {
        parsed
            .warnings
            .push(format!("ignored header without a colon: {line}"));
    }
}

/// `--data-urlencode`: `content`, `=content` or `name=content`; `@file` forms are not read.
fn urlencoded(value: &str, parsed: &mut Parsed) {
    // cURL looks for `=` first and only then for `@`.
    match value.find('=').or_else(|| value.find('@')) {
        Some(at) if value[at..].starts_with('@') => file_not_read(parsed, "--data-urlencode"),
        Some(0) => parsed.data.push(Data::Text(percent_encode(&value[1..]))),
        Some(at) => {
            let encoded = format!("{}={}", &value[..at], percent_encode(&value[at + 1..]));
            parsed.data.push(Data::Text(encoded));
        }
        None => parsed.data.push(Data::Text(percent_encode(value))),
    }
}

/// Percent-encodes everything except RFC 3986 unreserved characters, as cURL does.
pub(super) fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(byte).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

fn form(value: &str, typed: &str, parsed: &mut Parsed) {
    let Some((name, content)) = value.split_once('=') else {
        parsed
            .warnings
            .push(format!("ignored {typed} without `=`: {value}"));
        return;
    };
    let file = content
        .strip_prefix('@')
        .or_else(|| content.strip_prefix('<'));
    let part = match file {
        Some(path) => {
            if content.starts_with('<') {
                parsed
                    .warnings
                    .push(format!("{typed} {name}=<file is imported as a file upload"));
            }
            let path = path.split(';').next().unwrap_or(path);
            file_part(name, path)
        }
        None => text_part(name, content),
    };
    parsed.parts.push(part);
}

fn form_string(value: &str, typed: &str, parsed: &mut Parsed) {
    match value.split_once('=') {
        Some((name, content)) => parsed.parts.push(text_part(name, content)),
        None => parsed
            .warnings
            .push(format!("ignored {typed} without `=`: {value}")),
    }
}

fn text_part(name: &str, value: &str) -> Part {
    Part {
        name: name.to_string(),
        value: value.to_string(),
        file: None,
        enabled: true,
    }
}

fn file_part(name: &str, path: &str) -> Part {
    Part {
        name: name.to_string(),
        value: String::new(),
        file: Some(path.to_string()),
        enabled: true,
    }
}

fn max_time(value: &str, typed: &str, parsed: &mut Parsed) {
    match value.parse::<f64>() {
        Ok(seconds) if seconds.is_finite() && seconds >= 0.0 => {
            let ms = (seconds * MS_PER_SECOND).round().min(f64::from(u32::MAX));
            // In range and non-negative: checked above and clamped.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let ms = ms as u32;
            parsed.timeout_ms = Some(ms);
        }
        _ => parsed.warnings.push(format!(
            "ignored {typed}: `{value}` is not a number of seconds"
        )),
    }
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;
