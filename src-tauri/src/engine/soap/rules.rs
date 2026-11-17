//! Where a WSDL import may go (plan 06 §7.3 🔒). Pure checks over URLs and paths; the
//! walker applies them to every import and every redirect hop.

use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::{Component, Path, PathBuf};

use url::{Host, Url};

use crate::error::AppError;

const FILE_EXTENSIONS: [&str; 3] = ["wsdl", "xsd", "xml"];

fn refused(text: String) -> AppError {
    AppError::invalid_input(text)
}

/// `url` without user name or password, for reports and logs.
pub fn display_url(url: &Url) -> String {
    let mut shown = url.clone();
    // Only fails for URLs that cannot have credentials, which then have none.
    let _ = shown.set_password(None);
    let _ = shown.set_username("");
    shown.to_string()
}

/// `url` as listed in an import report: no credentials, no query, no fragment.
pub fn fetched_url(url: &Url) -> String {
    let mut shown = url.clone();
    shown.set_query(None);
    shown.set_fragment(None);
    display_url(&shown)
}

fn private_v4(ip: Ipv4Addr) -> bool {
    let [first, second, ..] = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || first == 0 // "this network" 0.0.0.0/8, incl. unspecified
        || (first == 100 && (second & 0xc0) == 64) // CGNAT 100.64.0.0/10
}

/// The IPv4 address carried by an IPv4-mapped, IPv4-compatible, NAT64 (64:ff9b::/96)
/// or 6to4 (2002::/16) IPv6 address.
fn embedded_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let s = ip.segments();
    let v4 = |high: u16, low: u16| {
        let [a, b] = high.to_be_bytes();
        let [c, d] = low.to_be_bytes();
        Ipv4Addr::new(a, b, c, d)
    };
    match s {
        [0, 0, 0, 0, 0, 0xffff, high, low] | [0, 0, 0, 0, 0, 0, high, low] => Some(v4(high, low)),
        [0x64, 0xff9b, 0, 0, 0, 0, high, low] => Some(v4(high, low)),
        [0x2002, high, low, ..] => Some(v4(high, low)),
        _ => None,
    }
}

fn private_v6(ip: Ipv6Addr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() {
        return true;
    }
    if let Some(v4) = embedded_v4(ip) {
        return private_v4(v4);
    }
    let first = ip.segments()[0];
    (first & 0xfe00) == 0xfc00 // unique local fc00::/7
        || (first & 0xffc0) == 0xfe80 // link-local fe80::/10
        || (first & 0xffc0) == 0xfec0 // site-local fec0::/10 (deprecated)
}

/// `localhost` or a loopback, private, link-local or unspecified literal address.
/// Hostnames are not resolved (accepted risk, plan 06 §11).
pub fn is_private_literal(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(ip)) => private_v4(ip),
        Some(Host::Ipv6(ip)) => private_v6(ip),
        Some(Host::Domain(name)) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            name == "localhost" || name.ends_with(".localhost")
        }
        None => false,
    }
}

pub fn same_origin(a: &Url, b: &Url) -> bool {
    a.origin() == b.origin()
}

/// An `http(s)` URL the walker may fetch for a root at `root` (`None`: a file root).
pub fn check_url(url: &Url, root: Option<&Url>) -> Result<(), AppError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(refused(format!(
            "Refused {}: only http and https imports are followed.",
            url.scheme()
        )));
    }
    let root_private = root.is_some_and(is_private_literal);
    let same = root.is_some_and(|root| same_origin(root, url));
    if !same && !root_private && is_private_literal(url) {
        return Err(refused(format!(
            "Refused {}: a local or private address cannot be imported from this WSDL.",
            display_url(url)
        )));
    }
    Ok(())
}

/// Resolves an import `location` of a URL document at `base`.
pub fn join_url(base: &Url, location: &str, root: Option<&Url>) -> Result<Url, AppError> {
    let url = base
        .join(location.trim())
        .map_err(|_| refused(format!("Refused \"{location}\": not a valid location.")))?;
    check_url(&url, root)?;
    Ok(url)
}

/// `path` with `.` and `..` folded without touching the disk; `None` when `..` climbs
/// above the start.
fn normalize(path: &Path) -> Option<PathBuf> {
    path.components().try_fold(PathBuf::new(), |mut out, part| {
        match part {
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
        Some(out)
    })
}

/// Resolves a relative import of the file `document`, confined to `root_dir` (the picked
/// file's directory) and to the WSDL/XSD extensions. Symlinks are checked when read.
pub fn join_file(document: &Path, location: &str, root_dir: &Path) -> Result<PathBuf, AppError> {
    let location = location.trim();
    let escapes = || {
        refused(format!(
            "Refused \"{location}\": it is outside the WSDL's folder."
        ))
    };
    if location.starts_with(['/', '\\'])
        || Path::new(location).is_absolute()
        || location.contains('\\')
    {
        return Err(escapes());
    }
    let joined = document.parent().unwrap_or(root_dir).join(location);
    let normalized = normalize(&joined).ok_or_else(escapes)?;
    if !normalized.starts_with(root_dir) {
        return Err(escapes());
    }
    let extension = normalized
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !FILE_EXTENSIONS.contains(&extension.as_str()) {
        return Err(refused(format!(
            "Refused \"{location}\": only .wsdl, .xsd and .xml files are imported."
        )));
    }
    Ok(normalized)
}

/// True when `location` is an absolute URL (any scheme) rather than a relative path.
pub fn is_absolute_url(location: &str) -> bool {
    Url::parse(location.trim()).is_ok_and(|url| url.scheme().len() > 1)
}

/// `path` shown relative to `root_dir` (never the absolute, machine-local path).
pub fn display_path(path: &Path, root_dir: &Path) -> String {
    path.strip_prefix(root_dir)
        .unwrap_or(path)
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Reads a file only when its canonical path (symlinks resolved) stays under the
/// canonical `root_dir`, it is a regular file, and it fits `max_bytes`.
pub async fn read_confined(
    path: &Path,
    root_dir: &Path,
    max_bytes: usize,
) -> Result<Vec<u8>, AppError> {
    let shown = display_path(path, root_dir);
    let canonical = tokio::fs::canonicalize(path)
        .await
        .map_err(|_| AppError::not_found(format!("Could not read {shown}.")))?;
    let root = tokio::fs::canonicalize(root_dir).await?;
    if !canonical.starts_with(&root) {
        return Err(refused(format!(
            "Refused {shown}: it is outside the WSDL's folder."
        )));
    }
    let metadata = tokio::fs::metadata(&canonical).await?;
    if !metadata.is_file() {
        return Err(refused(format!("Refused {shown}: not a file.")));
    }
    if metadata.len() > max_bytes as u64 {
        return Err(refused(format!(
            "{shown} is larger than {} MiB.",
            max_bytes / (1024 * 1024)
        )));
    }
    Ok(tokio::fs::read(&canonical).await?)
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
