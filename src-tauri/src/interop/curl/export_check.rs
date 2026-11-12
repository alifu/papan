//! What the exporter refuses: text that cURL would read as a file, a form type or an
//! option, and characters a POSIX shell cannot carry safely in single quotes.

use crate::error::AppError;
use crate::store::types::{ApiKeyPlacement, Auth, Body, RequestContent};

/// Characters that make `-F name=…` parse the name as more than a name.
const PART_NAME_SPECIALS: [char; 4] = ['=', ';', '@', '<'];
/// Characters that make `-F name=@path` parse the path as more than a path.
const FILE_PATH_SPECIALS: [char; 2] = [';', '"'];

fn refuse(message: &str) -> AppError {
    AppError::invalid_input(message)
}

/// Refuses content that cannot be copied as cURL faithfully and safely.
pub fn check_exportable(content: &RequestContent) -> Result<(), AppError> {
    let section = match (&content.body, &content.auth) {
        (Body::Unsupported, _) => Some("body"),
        (_, Auth::Unsupported) => Some("authentication"),
        _ => None,
    };
    if let Some(section) = section {
        return Err(AppError::invalid_input(format!(
            "This request's {section} was made by a newer Papan and cannot be copied as cURL."
        )));
    }
    check_header_names(content)?;
    check_parts(&content.body)
}

/// `-H @file` reads headers from a file; a `:` in a name would move the value.
fn check_header_names(content: &RequestContent) -> Result<(), AppError> {
    let api_key = match &content.auth {
        Auth::ApiKey {
            name,
            placement: ApiKeyPlacement::Header,
            ..
        } => Some(name.as_str()),
        _ => None,
    };
    let names = content
        .headers
        .iter()
        .filter(|row| row.enabled)
        .map(|row| row.name.as_str())
        .chain(api_key);
    for name in names {
        let name = name.trim();
        if name.starts_with('@') || name.contains(':') {
            return Err(refuse(
                "A header name starts with `@` or contains `:`, so it cannot be copied as cURL.",
            ));
        }
    }
    Ok(())
}

fn check_parts(body: &Body) -> Result<(), AppError> {
    let Body::Multipart { parts } = body else {
        return Ok(());
    };
    for part in parts.iter().filter(|part| part.enabled) {
        if part.name.contains(PART_NAME_SPECIALS) {
            return Err(refuse(
                "A form part name contains `=`, `;`, `@` or `<`, so it cannot be copied as cURL.",
            ));
        }
        if part
            .file
            .as_deref()
            .is_some_and(|file| file.contains(FILE_PATH_SPECIALS))
        {
            return Err(refuse(
                "A form file path contains `;` or `\"`, so it cannot be copied as cURL.",
            ));
        }
    }
    Ok(())
}

/// Newline, tab and carriage return travel literally inside `'…'`; any other control
/// character could be mangled or misread by a terminal or shell.
pub fn check_command(command: &str) -> Result<(), AppError> {
    let unsafe_control = |c: char| c.is_control() && !matches!(c, '\n' | '\t' | '\r');
    if command.chars().any(unsafe_control) {
        return Err(refuse(
            "This request contains a control character that cannot be copied safely as cURL.",
        ));
    }
    Ok(())
}
