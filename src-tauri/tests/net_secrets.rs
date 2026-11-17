//! Plan 03b §4.2, §6.5, §15: client certificates and persisted cookies. Nothing secret
//! reaches a file in clear; everything runs on `MemoryKeys` and generated certificates.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use aws_lc_rs::encoding::AsDer;
use base64::Engine as _;
use papan_lib::engine::http::types::{
    ClientCertificate, NetworkSettings, ProxyMode, ProxySettings,
};
use papan_lib::engine::http::{cookie_file, identity};
use papan_lib::error::{AppError, ErrorCode};
use papan_lib::vars::types::{SecretStorageMode, SecretStorageStatus};
use papan_lib::vars::vault::{
    sealed, FileVault, KeyError, MemoryKeys, MemoryVault, SecretChange, SecretStore,
};
use reqwest_cookie_store::CookieStore;
use url::Url;

#[path = "net_secrets/cookies.rs"]
mod cookies;

const PROXY_PASSWORD: &str = "pr0xy-PASSWORD-77";
const COOKIE_VALUE: &str = "c00kie-VALUE-4242";
const SESSION_VALUE: &str = "sess10n-VALUE-9";
const CERT_ID: &str = "0b0e7a52-6a55-4c3e-9d0e-2f1c8a9b7d11";
const JAR: &str = "collection-7f3c2b1a-9d8e-4f60-a1b2-c3d4e5f60718";
const COOKIE_KEY_ID: &str = "n/cookie-key";

struct AppData {
    dir: tempfile::TempDir,
    keys: MemoryKeys,
}

impl AppData {
    fn new() -> Self {
        Self::with_keys(MemoryKeys::new())
    }

    fn with_keys(keys: MemoryKeys) -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            keys,
        }
    }

    /// A new vault over the same file and key store, as after an app restart.
    fn vault(&self) -> FileVault<MemoryKeys> {
        FileVault::new(self.dir.path().join("secrets.vault"), self.keys.clone())
    }

    fn cookies(&self) -> PathBuf {
        self.dir.path().join("cookies")
    }

    fn jar_file(&self) -> PathBuf {
        self.cookies().join(format!("{JAR}.jar"))
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.dir.path().join(name);
        fs::write(&path, bytes).unwrap();
        path
    }
}

fn set(id: &str, value: &str) -> SecretChange {
    SecretChange::Set {
        id: id.to_string(),
        value: value.to_string(),
    }
}

fn pem(label: &str, der: &[u8]) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(der);
    let lines: Vec<&str> = encoded
        .as_bytes()
        .chunks(64)
        .map(|chunk| std::str::from_utf8(chunk).unwrap())
        .collect();
    format!(
        "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
        lines.join("\n")
    )
}

struct Pair {
    cert_pem: String,
    key_pem: String,
    key_der: Vec<u8>,
    sec1_pem: String,
}

/// A self-signed EC certificate with its key as PKCS#8 and as SEC1.
fn pair() -> Pair {
    let alg = &aws_lc_rs::signature::ECDSA_P256_SHA256_ASN1_SIGNING;
    let rng = aws_lc_rs::rand::SystemRandom::new();
    let pkcs8 = aws_lc_rs::signature::EcdsaKeyPair::generate_pkcs8(alg, &rng).unwrap();
    let key_der = pkcs8.as_ref().to_vec();
    let ec = aws_lc_rs::signature::EcdsaKeyPair::from_pkcs8(alg, &key_der).unwrap();
    let sec1 = ec.private_key().as_der().unwrap().as_ref().to_vec();
    let signer = rcgen::KeyPair::try_from(key_der.as_slice()).unwrap();
    let cert = rcgen::CertificateParams::new(vec!["client.example".to_string()])
        .unwrap()
        .self_signed(&signer)
        .unwrap();
    Pair {
        cert_pem: cert.pem(),
        key_pem: pem("PRIVATE KEY", &key_der),
        key_der,
        sec1_pem: pem("EC PRIVATE KEY", &sec1),
    }
}

fn files_containing(root: &Path, needle: &[u8]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(files_containing(&path, needle));
        } else if fs::read(&path)
            .unwrap()
            .windows(needle.len())
            .any(|window| window == needle)
        {
            found.push(path);
        }
    }
    found
}

fn jar_with(cookies: &[&str]) -> CookieStore {
    let url = Url::parse("https://api.example/").unwrap();
    let mut store = CookieStore::default();
    for cookie in cookies {
        store.parse(cookie, &url).unwrap();
    }
    store
}

fn persistent() -> CookieStore {
    jar_with(&[
        &format!("sid={COOKIE_VALUE}; Max-Age=3600"),
        &format!("tmp={SESSION_VALUE}"),
    ])
}

fn has(store: &CookieStore, name: &str) -> bool {
    store.contains("api.example", "/", name)
}

fn import_error(app: &AppData, cert: &[u8], key: Option<&[u8]>) -> AppError {
    let cert = app.write("client.pem", cert);
    let key = key.map(|bytes| app.write("client.key", bytes));
    identity::import(&cert, key.as_deref()).unwrap_err()
}

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

/// Counts the calls that would open the vault (and so prompt for the keychain).
#[derive(Default)]
struct Spy {
    inner: MemoryVault,
    reads: AtomicUsize,
}

impl SecretStore for Spy {
    fn get(&self, id: &str) -> Result<Option<String>, AppError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.get(id)
    }
    fn has(&self, id: &str) -> Result<bool, AppError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.has(id)
    }
    fn apply(&self, changes: &[SecretChange]) -> Result<(), AppError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.apply(changes)
    }
    fn delete_prefix(&self, prefix: &str) -> Result<(), AppError> {
        self.inner.delete_prefix(prefix)
    }
    fn status(&self) -> SecretStorageStatus {
        self.inner.status()
    }
    fn reset(&self) -> Result<(), AppError> {
        self.inner.reset()
    }
}

// ---- Nothing secret in clear ----------------------------------------------------------

#[test]
fn only_the_vault_file_holds_the_password_the_key_and_the_cookies_and_none_in_clear() {
    let app = AppData::new();
    let pair = pair();
    let vault = app.vault();
    let cert = app.write("picked.pem", pair.cert_pem.as_bytes());
    let key = app.write("picked.key", pair.key_pem.as_bytes());

    vault
        .apply(&[set("n/proxy/password", PROXY_PASSWORD)])
        .unwrap();
    let bundle = identity::import(&cert, Some(&key)).unwrap();
    identity::store(&vault, CERT_ID, &bundle).unwrap();
    cookie_file::save(&app.cookies(), JAR, &vault, &persistent()).unwrap();
    fs::remove_file(cert).unwrap();
    fs::remove_file(key).unwrap();

    let key_line = pair.key_pem.lines().nth(1).unwrap().as_bytes();
    for needle in [
        PROXY_PASSWORD.as_bytes(),
        key_line,
        &pair.key_der,
        COOKIE_VALUE.as_bytes(),
        SESSION_VALUE.as_bytes(),
    ] {
        assert_eq!(
            files_containing(app.dir.path(), needle),
            Vec::<PathBuf>::new()
        );
    }
    assert!(app.jar_file().is_file());
    let settings = NetworkSettings {
        proxy: ProxySettings {
            mode: ProxyMode::Custom,
            url: "http://proxy.corp:3128".to_string(),
            username: "alifu".to_string(),
            has_password: vault.has("n/proxy/password").unwrap(),
            bypass: Vec::new(),
        },
        certificates: vec![entry(CERT_ID, "api.bank.example", Some(8443))],
        storage: vault.status(),
    };
    let json = serde_json::to_string(&settings).unwrap();
    assert!(!json.contains(PROXY_PASSWORD) && !json.contains("PRIVATE KEY"));
    assert!(!json.contains(pair.key_pem.lines().nth(1).unwrap()));
}

// ---- Client certificates --------------------------------------------------------------

#[test]
fn import_keeps_the_chain_and_the_one_key_and_drops_everything_else() {
    let app = AppData::new();
    let pair = pair();
    let params = pem(
        "EC PARAMETERS",
        &[0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 3, 1, 7],
    );
    let combined = format!("{params}{}\n{}", pair.sec1_pem, pair.cert_pem);
    let single = app.write("combined.pem", combined.as_bytes());

    let bundle = identity::import(&single, None).unwrap();

    assert!(bundle.starts_with("-----BEGIN CERTIFICATE-----"));
    assert_eq!(bundle.matches("-----BEGIN EC PRIVATE KEY-----").count(), 1);
    assert!(!bundle.contains("EC PARAMETERS"));
}

#[test]
fn unusable_certificate_files_are_refused_with_a_fixed_message() {
    let app = AppData::new();
    let (ours, other) = (pair(), pair());
    let encrypted = pem("ENCRYPTED PRIVATE KEY", &ours.key_der);
    let legacy = ours.sec1_pem.replacen(
        "-----\n",
        "-----\nProc-Type: 4,ENCRYPTED\nDEK-Info: AES-256-CBC,00\n\n",
        1,
    );
    let two_keys = format!("{}{}", ours.key_pem, other.key_pem);
    let huge = vec![b'A'; 1024 * 1024];
    let cert = ours.cert_pem.as_bytes();
    type Case<'a> = (&'a str, &'a [u8], Option<&'a [u8]>, &'a str);
    let cases: [Case; 8] = [
        (
            "pkcs8 passphrase",
            cert,
            Some(encrypted.as_bytes()),
            "passphrase-protected",
        ),
        (
            "legacy passphrase",
            cert,
            Some(legacy.as_bytes()),
            "passphrase-protected",
        ),
        (
            "mismatched key",
            cert,
            Some(other.key_pem.as_bytes()),
            "cannot be used with this",
        ),
        ("certificate only", cert, None, "No private key"),
        (
            "no certificate",
            ours.key_pem.as_bytes(),
            None,
            "No certificate",
        ),
        (
            "two keys",
            cert,
            Some(two_keys.as_bytes()),
            "More than one private key",
        ),
        ("1 MiB", &huge, None, "too large"),
        ("not PEM", &[0xff, 0xfe, 0x00], None, "No certificate"),
    ];

    for (name, cert_file, key_file, expected) in cases {
        let error = import_error(&app, cert_file, key_file);

        assert_eq!(error.code, ErrorCode::InvalidInput, "{name}");
        assert!(
            error.message.contains(expected),
            "{name}: {}",
            error.message
        );
        let shown = format!("{} {:?}", error.message, error.detail);
        assert!(!shown.contains(app.dir.path().to_str().unwrap()), "{name}");
    }
    assert!(!app.dir.path().join("secrets.vault").exists());
}

#[test]
fn a_stored_certificate_loads_after_a_restart_and_is_gone_after_remove() {
    let app = AppData::new();
    let pair = pair();
    let cert = app.write("client.pem", pair.cert_pem.as_bytes());
    let key = app.write("client.key", pair.key_pem.as_bytes());
    let bundle = identity::import(&cert, Some(&key)).unwrap();
    identity::store(&app.vault(), CERT_ID, &bundle).unwrap();

    let loaded = identity::load(&app.vault(), CERT_ID).unwrap();
    identity::remove(&app.vault(), CERT_ID).unwrap();

    assert!(loaded.is_some());
    assert!(identity::load(&app.vault(), CERT_ID).unwrap().is_none());
    assert!(!app.vault().has(&format!("n/cert/{CERT_ID}")).unwrap());
}
