//! Safe XML reading for WSDL imports (plan 06 §7.1): the size is checked before anything
//! is decoded, a DTD is refused outright (no external entities, no entity expansion), and
//! the node count is capped. UTF-8 and UTF-16 (with a BOM, or sniffed from `<`) are read;
//! other encodings named in the XML declaration are decoded through `encoding_rs`.

use encoding_rs::{Encoding, UTF_16BE, UTF_16LE, UTF_8};
use roxmltree::{Document, Node, ParsingOptions};

use super::model::QName;
use crate::error::AppError;

/// Per document cap (§7.3).
pub const MAX_DOCUMENT_BYTES: usize = 10 * 1024 * 1024;
pub const NODES_LIMIT: u32 = 1_000_000;

pub const DTD_REFUSED: &str = "DTDs are not allowed in WSDL imports.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XmlLimits {
    pub max_bytes: usize,
    pub nodes_limit: u32,
}

impl Default for XmlLimits {
    fn default() -> Self {
        Self {
            max_bytes: MAX_DOCUMENT_BYTES,
            nodes_limit: NODES_LIMIT,
        }
    }
}

fn too_large(limit: usize) -> AppError {
    AppError::invalid_input(format!(
        "The document is larger than {} MiB.",
        limit / (1024 * 1024)
    ))
}

/// The encoding a document starts with: a BOM, then a sniffed UTF-16 `<`.
fn sniff(bytes: &[u8]) -> Option<(&'static Encoding, usize)> {
    if let Some((encoding, bom)) = Encoding::for_bom(bytes) {
        return Some((encoding, bom));
    }
    match bytes {
        [0, b'<', ..] => Some((UTF_16BE, 0)),
        [b'<', 0, ..] => Some((UTF_16LE, 0)),
        _ => None,
    }
}

/// The `encoding="…"` of an ASCII-compatible XML declaration.
fn declared(bytes: &[u8]) -> Option<&'static Encoding> {
    let head = &bytes[..bytes.len().min(200)];
    let ascii = head
        .iter()
        .position(|b| !b.is_ascii())
        .unwrap_or(head.len());
    let head = std::str::from_utf8(&head[..ascii]).ok()?;
    let declaration = head.strip_prefix("<?xml")?.split("?>").next()?;
    let after = declaration.split("encoding").nth(1)?;
    let value = after.trim_start().strip_prefix('=')?.trim_start();
    let quote = value.chars().next()?;
    let label = value.get(1..)?.split(quote).next()?;
    Encoding::for_label(label.as_bytes())
}

/// Decodes `bytes` to text, refusing anything over `max_bytes` before decoding.
pub fn decode(bytes: &[u8], max_bytes: usize) -> Result<String, AppError> {
    if bytes.len() > max_bytes {
        return Err(too_large(max_bytes));
    }
    // A label naming UTF-16 on an ASCII-compatible file is a lie: it is read as UTF-8.
    let (encoding, skip) = sniff(bytes).unwrap_or_else(|| {
        let label = declared(bytes).filter(|e| *e != UTF_16LE && *e != UTF_16BE);
        (label.unwrap_or(UTF_8), 0)
    });
    encoding
        .decode_without_bom_handling_and_without_replacement(&bytes[skip..])
        .map(|text| text.into_owned())
        .ok_or_else(|| {
            AppError::invalid_input(format!("The document is not valid {}.", encoding.name()))
        })
}

/// True when the prolog (before the root element) holds a `<!DOCTYPE`.
fn has_doctype(text: &str) -> bool {
    let mut rest = text.trim_start_matches('\u{feff}');
    loop {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix("<?") {
            match after.find("?>") {
                Some(end) => rest = &after[end + 2..],
                None => return false,
            }
        } else if let Some(after) = rest.strip_prefix("<!--") {
            match after.find("-->") {
                Some(end) => rest = &after[end + 3..],
                None => return false,
            }
        } else {
            return rest
                .get(..9)
                .is_some_and(|head| head.eq_ignore_ascii_case("<!DOCTYPE"));
        }
    }
}

/// Parses decoded text with DTDs refused and the node count capped.
pub fn parse(text: &str, limits: XmlLimits) -> Result<Document<'_>, AppError> {
    if text.len() > limits.max_bytes {
        return Err(too_large(limits.max_bytes));
    }
    if has_doctype(text) {
        return Err(AppError::invalid_input(DTD_REFUSED));
    }
    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit: limits.nodes_limit,
        ..ParsingOptions::default()
    };
    Document::parse_with_options(text, options).map_err(|error| match error {
        roxmltree::Error::DtdDetected => AppError::invalid_input(DTD_REFUSED),
        roxmltree::Error::NodesLimitReached => {
            AppError::invalid_input("The document has too many XML nodes.")
        }
        other => AppError::invalid_input("The document is not well-formed XML.")
            .with_detail(other.to_string()),
    })
}

/// The element children of `node`.
pub fn elements<'a, 'input>(node: Node<'a, 'input>) -> impl Iterator<Item = Node<'a, 'input>> {
    node.children().filter(Node::is_element)
}

/// The element children of `node` named `{ns}name`.
pub fn children_named<'a, 'input>(
    node: Node<'a, 'input>,
    ns: &'a str,
    name: &'a str,
) -> impl Iterator<Item = Node<'a, 'input>> {
    elements(node).filter(move |child| child.has_tag_name((ns, name)))
}

/// The first element child of `node` named `{ns}name`.
pub fn child_named<'a, 'input>(
    node: Node<'a, 'input>,
    ns: &'a str,
    name: &'a str,
) -> Option<Node<'a, 'input>> {
    children_named(node, ns, name).next()
}

/// Resolves a QName attribute value (`prefix:local`) with the namespaces in scope at
/// `node`. An unprefixed value takes the default namespace, if any.
pub fn resolve_qname(node: Node<'_, '_>, value: &str) -> Result<QName, AppError> {
    let value = value.trim();
    let (prefix, local) = match value.split_once(':') {
        Some((prefix, local)) => (Some(prefix), local),
        None => (None, value),
    };
    if !is_ncname(local) || prefix.is_some_and(|prefix| !is_ncname(prefix)) {
        return Err(invalid_name(value));
    }
    let ns = match node.lookup_namespace_uri(prefix) {
        Some(ns) => ns,
        None if prefix.is_none() => "",
        None => {
            return Err(AppError::invalid_input(format!(
                "The prefix of \"{}\" is not declared.",
                neutralise(value)
            )))
        }
    };
    Ok(QName::new(ns, local))
}

/// The `name` attribute of `node`, or an error naming what it describes. A name that is
/// not an NCName is refused: names become tags, so they must not carry markup.
pub fn required_name<'a>(node: Node<'a, '_>, what: &str) -> Result<&'a str, AppError> {
    let name = node
        .attribute("name")
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| AppError::invalid_input(format!("A {what} has no name.")))?;
    if is_ncname(name) {
        Ok(name)
    } else {
        Err(invalid_name(name))
    }
}

/// An XML NCName: a letter or `_`, then letters, digits, `.`, `-`, `_` or `·`. No
/// markup, quotes, braces, whitespace or `:` can appear in one.
pub fn is_ncname(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_')
        && chars.all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | '\u{b7}'))
}

/// `{{` escaped as `\{{`, so text taken from a WSDL never resolves as a variable when the
/// request is sent (plan 04 §5: `\{{` stands for a literal `{{`).
pub fn neutralise(text: &str) -> String {
    text.replace("{{", "\\{{")
}

fn invalid_name(name: &str) -> AppError {
    let shown: String = name.chars().take(60).collect();
    AppError::invalid_input(format!(
        "\"{}\" is not a valid XML name and was skipped.",
        neutralise(&shown)
    ))
}

#[cfg(test)]
#[path = "xml_tests.rs"]
mod tests;
