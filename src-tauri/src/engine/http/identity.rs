//! Client certificates (plan 03b §4.2, §15.3): import validation, the vault value
//! `n/cert/<id>`, and the rule that picks an entry for one hop.
//!
//! Every error text here is fixed: no parser or TLS error text and no file path, because
//! either could quote key material (§15.2). `network.json`, the dialogs and the client
//! selector belong to the callers.

use std::io::Read;
use std::path::Path;

use reqwest::Identity;
use url::Url;
use zeroize::Zeroizing;

use super::types::ClientCertificate;
use crate::error::AppError;
use crate::vars::vault::{SecretChange, SecretStore};

/// The two picked files together, and the stored bundle, may not be larger than this.
pub const MAX_BUNDLE_BYTES: usize = 64 * 1024;

const ID_PREFIX: &str = "n/cert/";
const BEGIN: &str = "-----BEGIN ";
const END: &str = "-----END ";
const DASHES: &str = "-----";
const CERTIFICATE: &str = "CERTIFICATE";
const KEY_LABELS: [&str; 3] = ["PRIVATE KEY", "RSA PRIVATE KEY", "EC PRIVATE KEY"];
const ENCRYPTED_KEY: &str = "ENCRYPTED PRIVATE KEY";
/// The header line of a passphrase-protected legacy (PKCS#1, SEC1) key.
const LEGACY_ENCRYPTED: &str = "Proc-Type: 4,ENCRYPTED";

const TOO_LARGE: &str = "The certificate and key are too large (64 KiB at most).";
const UNREADABLE_FILE: &str = "The file cannot be read.";
const PASSPHRASE: &str = "The key is passphrase-protected. Decrypt it first: \
    `openssl pkey -in key.pem -out plain.pem`.";
const NO_CERTIFICATE: &str = "No certificate was found. Choose a PEM certificate file.";
const NO_KEY: &str = "No private key was found. Choose the PEM key file.";
const MANY_KEYS: &str = "More than one private key was found. Use a file with one key.";
const NOT_READABLE: &str = "The certificate or the key cannot be read.";
const KEY_MISMATCH: &str = "The key cannot be used with this certificate.";
const BAD_ID: &str = "The client certificate id is not valid.";
const BAD_STORED: &str = "The stored client certificate cannot be read. Import it again.";
const BAD_HOST: &str = "Enter a host name without scheme, port or path.";

fn invalid(message: &str) -> AppError {
    AppError::invalid_input(message)
}

/// The PEM sections of one file that matter for an identity.
#[derive(Default)]
struct Sections {
    certificates: Vec<Zeroizing<String>>,
    keys: Vec<Zeroizing<String>>,
    encrypted: bool,
}

/// Splits `text` into its `CERTIFICATE` and private-key sections, each rebuilt as label
/// lines around the trimmed body lines. Other sections (`EC PARAMETERS`, …) and text
/// outside sections are dropped.
fn scan(text: &str) -> Sections {
    let mut sections = Sections::default();
    let mut open: Option<(&str, Zeroizing<String>)> = None;
    for line in text.lines().map(str::trim) {
        if let Some(label) = line
            .strip_prefix(BEGIN)
            .and_then(|rest| rest.strip_suffix(DASHES))
        {
            sections.encrypted |= label == ENCRYPTED_KEY;
            open = Some((label, Zeroizing::new(format!("{line}\n"))));
            continue;
        }
        let Some((label, mut body)) = open.take() else {
            continue;
        };
        sections.encrypted |= line.starts_with(LEGACY_ENCRYPTED);
        if line.is_empty() {
            open = Some((label, body));
            continue;
        }
        body.push_str(line);
        body.push('\n');
        if !line.starts_with(END) {
            open = Some((label, body));
        } else if label == CERTIFICATE {
            sections.certificates.push(body);
        } else if KEY_LABELS.contains(&label) {
            sections.keys.push(body);
        }
    }
    sections
}

/// Reads at most `limit` bytes; a longer file is refused without being read whole.
fn read_limited(path: &Path, limit: usize) -> Result<Zeroizing<Vec<u8>>, AppError> {
    let file = std::fs::File::open(path).map_err(|_| invalid(UNREADABLE_FILE))?;
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid(UNREADABLE_FILE))?;
    if bytes.len() > limit {
        return Err(invalid(TOO_LARGE));
    }
    Ok(bytes)
}

fn scan_bytes(bytes: &[u8]) -> Sections {
    // A file that is not text holds no PEM section.
    std::str::from_utf8(bytes).map(scan).unwrap_or_default()
}

/// Requires reqwest to parse the bundle and rustls to accept the key for the leaf
/// certificate (§15.3.1: `from_pem` alone only splits the sections).
fn check_usable(bundle: &str) -> Result<(), AppError> {
    let identity = Identity::from_pem(bundle.as_bytes()).map_err(|_| invalid(NOT_READABLE))?;
    reqwest::Client::builder()
        .identity(identity)
        .build()
        .map(|_| ())
        .map_err(|_| invalid(KEY_MISMATCH))
}

/// Validates the picked files and returns the bundle to store: the certificates of
/// `cert_file` in file order, then the one unencrypted private key found in either file.
pub fn import(cert_file: &Path, key_file: Option<&Path>) -> Result<Zeroizing<String>, AppError> {
    let cert_bytes = read_limited(cert_file, MAX_BUNDLE_BYTES)?;
    let key_bytes = key_file
        .map(|path| read_limited(path, MAX_BUNDLE_BYTES - cert_bytes.len()))
        .transpose()?;
    let from_cert = scan_bytes(&cert_bytes);
    let from_key = key_bytes
        .as_deref()
        .map(|b| scan_bytes(b))
        .unwrap_or_default();
    if from_cert.encrypted || from_key.encrypted {
        return Err(invalid(PASSPHRASE));
    }
    if from_cert.certificates.is_empty() {
        return Err(invalid(NO_CERTIFICATE));
    }
    let keys: Vec<_> = from_cert.keys.iter().chain(&from_key.keys).collect();
    let key = match keys.as_slice() {
        [] => return Err(invalid(NO_KEY)),
        [key] => key,
        _ => return Err(invalid(MANY_KEYS)),
    };
    let mut bundle = Zeroizing::new(String::new());
    for section in from_cert.certificates.iter().chain([*key]) {
        bundle.push_str(section);
    }
    if bundle.len() > MAX_BUNDLE_BYTES {
        return Err(invalid(TOO_LARGE));
    }
    check_usable(&bundle)?;
    Ok(bundle)
}

/// The vault id of a certificate; `id` must be a lowercase hyphenated UUID.
fn vault_id(id: &str) -> Result<String, AppError> {
    let canonical =
        uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.as_hyphenated().to_string() == id);
    if !canonical {
        return Err(invalid(BAD_ID));
    }
    Ok(format!("{ID_PREFIX}{id}"))
}

/// Stores a bundle returned by `import` as `n/cert/<id>`.
pub fn store(vault: &dyn SecretStore, id: &str, bundle: &str) -> Result<(), AppError> {
    vault.apply(&[SecretChange::Set {
        id: vault_id(id)?,
        value: bundle.to_string(),
    }])
}

pub fn remove(vault: &dyn SecretStore, id: &str) -> Result<(), AppError> {
    vault.apply(&[SecretChange::Delete { id: vault_id(id)? }])
}

/// The stored identity, or `None` when the vault holds no value for `id`.
pub fn load(vault: &dyn SecretStore, id: &str) -> Result<Option<Identity>, AppError> {
    let Some(bundle) = vault.get(&vault_id(id)?)?.map(Zeroizing::new) else {
        return Ok(None);
    };
    Identity::from_pem(bundle.as_bytes())
        .map(Some)
        .map_err(|_| invalid(BAD_STORED))
}

/// The identity of an entry that matched a hop. A missing vault value fails the hop: the
/// request is never sent without the certificate its host was given (§15.3.3).
pub fn load_required(
    vault: &dyn SecretStore,
    entry: &ClientCertificate,
) -> Result<Identity, AppError> {
    load(vault, &entry.id)?.ok_or_else(|| {
        AppError::invalid_input(format!(
            "The client certificate for {} is missing. Import it again.",
            entry.host
        ))
    })
}

/// The form in which a certificate's host is stored: what `Url::host_str()` reports for
/// it (lowercase, punycode, IPv6 in brackets). No wildcard, port, userinfo or path.
pub fn normalize_host(host: &str) -> Result<String, AppError> {
    let url = Url::parse(&format!("https://{}/", host.trim())).map_err(|_| invalid(BAD_HOST))?;
    let bare = url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none();
    match url.host_str() {
        Some(normal) if bare && !normal.contains('*') => Ok(normal.to_string()),
        _ => Err(invalid(BAD_HOST)),
    }
}

/// The entry whose certificate one hop to `url` presents: `https` only, exact host, and
/// the entry's port (absent = any). An entry with the port wins over one without. The
/// token endpoint of an OAuth configuration is matched like any other hop (§15.4.1).
pub fn match_entry<'a>(
    entries: &'a [ClientCertificate],
    url: &Url,
) -> Option<&'a ClientCertificate> {
    if url.scheme() != "https" {
        return None;
    }
    let host = url.host_str()?;
    let port = url.port_or_known_default();
    let on_host = |entry: &&ClientCertificate| entry.host.eq_ignore_ascii_case(host);
    entries
        .iter()
        .filter(on_host)
        .find(|entry| entry.port.is_some() && entry.port == port)
        .or_else(|| {
            entries
                .iter()
                .filter(on_host)
                .find(|entry| entry.port.is_none())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use crate::vars::vault::MemoryVault;

    const CERT_ID: &str = "0b0e7a52-6a55-4c3e-9d0e-2f1c8a9b7d11";

    fn entry(id: &str, host: &str, port: Option<u16>) -> ClientCertificate {
        ClientCertificate {
            id: id.to_string(),
            host: host.to_string(),
            port,
            cert_name: "client.pem".to_string(),
            key_name: "client.key".to_string(),
            added_at_ms: 0.0,
            missing: false,
        }
    }

    fn set(id: &str, value: &str) -> SecretChange {
        SecretChange::Set {
            id: id.to_string(),
            value: value.to_string(),
        }
    }

    #[test]
    fn a_matching_entry_without_its_vault_value_fails_instead_of_sending_bare() {
        let vault = MemoryVault::default();
        let missing = entry(CERT_ID, "api.bank.example", None);

        let error = load_required(&vault, &missing).unwrap_err();

        assert_eq!(error.code, ErrorCode::InvalidInput);
        assert_eq!(
            error.message,
            "The client certificate for api.bank.example is missing. Import it again."
        );
    }

    #[test]
    fn certificate_ids_that_are_not_generated_uuids_are_rejected() {
        let vault = MemoryVault::default();

        for id in [
            "",
            "../x",
            "proxy/password",
            "0B0E7A52-6A55-4C3E-9D0E-2F1C8A9B7D11",
        ] {
            assert!(store(&vault, id, "bundle").is_err(), "{id}");
            assert!(load(&vault, id).is_err(), "{id}");
            assert!(remove(&vault, id).is_err(), "{id}");
        }
        vault
            .apply(&[set(&format!("n/cert/{CERT_ID}"), "not a bundle")])
            .unwrap();
        assert!(load(&vault, CERT_ID).is_err());
    }

    #[test]
    fn an_entry_matches_https_hops_to_its_own_host_and_port_only() {
        let other_id = "11111111-2222-4333-8444-555555555555";
        let entries = [
            entry(CERT_ID, "api.bank.example", None),
            entry(other_id, "api.bank.example", Some(8443)),
            entry(other_id, "[::1]", Some(9443)),
        ];
        let cases = [
            ("https://api.bank.example/token", Some(CERT_ID)),
            ("https://API.Bank.Example:443/", Some(CERT_ID)),
            ("https://api.bank.example:8443/", Some(other_id)),
            ("https://[::1]:9443/", Some(other_id)),
            ("http://api.bank.example/", None),
            ("https://evil.example/?h=api.bank.example", None),
            ("https://sub.api.bank.example/", None),
            ("https://[::1]/", None),
        ];

        for (url, expected) in cases {
            let matched = match_entry(&entries, &Url::parse(url).unwrap());

            assert_eq!(matched.map(|entry| entry.id.as_str()), expected, "{url}");
        }
    }

    #[test]
    fn a_host_is_stored_in_the_form_a_url_reports_it() {
        assert_eq!(
            normalize_host(" API.Bank.Example ").unwrap(),
            "api.bank.example"
        );
        assert_eq!(
            normalize_host("bücher.example").unwrap(),
            "xn--bcher-kva.example"
        );
        assert_eq!(normalize_host("[::1]").unwrap(), "[::1]");
        for host in [
            "",
            "a.example:8443",
            "user@a.example",
            "a.example/x",
            "a.example?x",
            "*.a.example",
        ] {
            assert!(normalize_host(host).is_err(), "{host}");
        }
    }
}
