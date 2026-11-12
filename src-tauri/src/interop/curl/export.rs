//! A request to a `curl` command (plan 03b §7). Pure: the caller decides what the content
//! holds (see [`super::masked`]). One option per line, `-X` always, the URL last.

use super::export_check::{check_command, check_exportable};
use super::quote::{bare_or_quoted, quote};
use crate::error::AppError;
use crate::store::types::{
    ApiKeyPlacement, Auth, Body, HttpVersion, KeyValue, Part, RequestContent, Settings,
    DEFAULT_TIMEOUT_MS,
};

/// What stands in for an OAuth 2 access token: the export never holds one.
pub const OAUTH_TOKEN_PLACEHOLDER: &str = "<access token>";
const LINE_BREAK: &str = " \\\n  ";
/// Papan's own types for a text and an XML body (`engine/http/build.rs`).
const TEXT_TYPE: &str = "text/plain; charset=utf-8";
const XML_TYPE: &str = "application/xml";
const JSON_TYPE: &str = "application/json";
const GLOB_CHARACTERS: [char; 4] = ['{', '}', '[', ']'];

/// The command for `content`, exactly as given; refused when cURL would read part of it
/// as a file or an option, or a shell could misread it.
pub fn export(content: &RequestContent) -> Result<String, AppError> {
    check_exportable(content)?;
    let url = with_api_key_query(&content.url, &content.auth);
    let mut lines = vec![format!("curl -X {}", bare_or_quoted(&content.method))];
    if url.contains(GLOB_CHARACTERS) {
        lines.push("-g".to_string());
    }
    lines.extend(content.headers.iter().filter_map(header_line));
    lines.extend(content_type_line(content));
    lines.extend(auth_lines(&content.auth));
    lines.extend(body_lines(&content.body));
    lines.extend(settings_lines(&content.settings));
    lines.push(url_argument(&url));
    let command = lines.join(LINE_BREAK);
    check_command(&command)?;
    Ok(command)
}

/// A URL starting with `-` would be read as an option.
fn url_argument(url: &str) -> String {
    if url.starts_with('-') {
        format!("--url {}", quote(url))
    } else {
        quote(url)
    }
}

fn header_option(name: &str, value: &str) -> String {
    // `-H 'Name:'` would remove the header in cURL; `Name;` sends it empty.
    if value.is_empty() {
        format!("-H {}", quote(&format!("{name};")))
    } else {
        format!("-H {}", quote(&format!("{name}: {value}")))
    }
}

/// cURL sends `-d`-style data as a form; this makes it send what Papan sends.
fn content_type_line(content: &RequestContent) -> Option<String> {
    let given = content
        .headers
        .iter()
        .any(|row| row.enabled && row.name.trim().eq_ignore_ascii_case("content-type"));
    let papan_type = match &content.body {
        Body::Json { .. } => JSON_TYPE,
        Body::Text { .. } => TEXT_TYPE,
        Body::Xml { .. } => XML_TYPE,
        // Papan sends a file body without a type; `Content-Type:` removes cURL's.
        Body::Binary { file: Some(path) } if !path.is_empty() => "",
        _ => return None,
    };
    if given {
        return None;
    }
    Some(format!(
        "-H {}",
        quote(&format!("Content-Type:{}", prefixed(papan_type)))
    ))
}

fn prefixed(value: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        format!(" {value}")
    }
}

fn header_line(row: &KeyValue) -> Option<String> {
    let name = row.name.trim();
    (row.enabled && !name.is_empty()).then(|| header_option(name, &row.value))
}

fn auth_lines(auth: &Auth) -> Vec<String> {
    match auth {
        Auth::Basic { username, password } => {
            vec![format!("-u {}", quote(&format!("{username}:{password}")))]
        }
        Auth::Digest { username, password } => {
            vec![format!(
                "--digest -u {}",
                quote(&format!("{username}:{password}"))
            )]
        }
        Auth::Bearer { token } => vec![header_option("Authorization", &format!("Bearer {token}"))],
        Auth::ApiKey {
            name,
            value,
            placement: ApiKeyPlacement::Header,
        } if !name.trim().is_empty() => vec![header_option(name.trim(), value)],
        Auth::Oauth2 { .. } => vec![header_option(
            "Authorization",
            &format!("Bearer {OAUTH_TOKEN_PLACEHOLDER}"),
        )],
        Auth::ApiKey { .. } | Auth::None | Auth::Unsupported => Vec::new(),
    }
}

fn with_api_key_query(url: &str, auth: &Auth) -> String {
    let Auth::ApiKey {
        name,
        value,
        placement: ApiKeyPlacement::Query,
    } = auth
    else {
        return url.to_string();
    };
    if name.is_empty() {
        return url.to_string();
    }
    let encode =
        |text: &str| -> String { url::form_urlencoded::byte_serialize(text.as_bytes()).collect() };
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}{}={}", encode(name), encode(value))
}

fn body_lines(body: &Body) -> Vec<String> {
    match body {
        // Not `--json`: it would read a body starting with `@` as a file.
        Body::Json { text } | Body::Xml { text } | Body::Text { text } => {
            vec![format!("--data-raw {}", quote(text))]
        }
        Body::Form { fields } => fields
            .iter()
            .filter(|field| field.enabled && !field.name.is_empty())
            .map(|field| {
                let pair = format!("{}={}", field.name, field.value);
                format!("--data-urlencode {}", quote(&pair))
            })
            .collect(),
        Body::Multipart { parts } => parts
            .iter()
            .filter(|part| part.enabled && !part.name.is_empty())
            .map(part_line)
            .collect(),
        Body::Binary { file: Some(path) } if !path.is_empty() => {
            vec![format!("--data-binary @{}", bare_or_quoted(path))]
        }
        Body::Binary { .. } | Body::None | Body::Unsupported => Vec::new(),
    }
}

fn part_line(part: &Part) -> String {
    if let Some(file) = part.file.as_deref().filter(|file| !file.is_empty()) {
        return format!("-F {}", quote(&format!("{}=@{file}", part.name)));
    }
    // `-F` would read `@`/`<` values as files and `;type=` as a type.
    format!(
        "--form-string {}",
        quote(&format!("{}={}", part.name, part.value))
    )
}

fn settings_lines(settings: &Settings) -> Vec<String> {
    let mut lines = Vec::new();
    if settings.follow_redirects {
        lines.push("-L".to_string());
    }
    if !settings.verify_tls {
        lines.push("-k".to_string());
    }
    if settings.timeout_ms != DEFAULT_TIMEOUT_MS {
        lines.push(format!("-m {}", seconds(settings.timeout_ms)));
    }
    match settings.http_version {
        HttpVersion::Http1 => lines.push("--http1.1".to_string()),
        HttpVersion::Http2 => lines.push("--http2".to_string()),
        HttpVersion::Auto => {}
    }
    lines
}

fn seconds(ms: u32) -> String {
    if ms.is_multiple_of(1000) {
        (ms / 1000).to_string()
    } else {
        (f64::from(ms) / 1000.0).to_string()
    }
}

#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;
