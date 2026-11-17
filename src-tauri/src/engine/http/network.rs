//! Machine-local network settings (plan 03b §4, §15): `<app data>/network.json` holds the
//! proxy and the list of client certificates; the proxy password (`n/proxy/password`)
//! and the certificate bundles (`n/cert/<id>`) live in the vault only. Collection files
//! never carry any of this.
//!
//! Every write bumps the generation, so clients built from older settings are never
//! reused (`client::ClientKey::network_generation`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::identity;
use super::mask::mask_secrets;
use super::proxy::{self, StoredProxy};
use super::proxy_secret::{password_changes, password_of};
use super::types::{ClientCertificate, NetworkSettings, ProxyInput, ProxyMode, ProxySettings};
use super::variables::now_ms;
use crate::error::AppError;
use crate::store::fsio;
use crate::vars::types::SecretEdit;
use crate::vars::vault::SecretStore;

pub const NETWORK_FILE: &str = "network.json";
pub use super::proxy_secret::{PROXY_ENDPOINT_ID, PROXY_PASSWORD_ID};
pub const MAX_CERTIFICATES: usize = 50;
const FILE_VERSION: u32 = 1;

const TOO_MANY_CERTIFICATES: &str = "At most 50 client certificates can be added.";
const UNKNOWN_CERTIFICATE: &str = "That client certificate no longer exists.";

/// One certificate as listed in `network.json`: display data only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct StoredCertificate {
    id: String,
    host: String,
    port: Option<u16>,
    cert_name: String,
    key_name: String,
    added_at_ms: f64,
}

impl StoredCertificate {
    fn shown(&self, missing: bool) -> ClientCertificate {
        ClientCertificate {
            id: self.id.clone(),
            host: self.host.clone(),
            port: self.port,
            cert_name: self.cert_name.clone(),
            key_name: self.key_name.clone(),
            added_at_ms: self.added_at_ms,
            missing,
        }
    }

    fn is_valid(&self) -> bool {
        uuid::Uuid::parse_str(&self.id).is_ok_and(|id| id.as_hyphenated().to_string() == self.id)
            && identity::normalize_host(&self.host).is_ok_and(|host| host == self.host)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct NetworkFile {
    version: u32,
    #[serde(default)]
    proxy: StoredProxy,
    #[serde(default)]
    certificates: Vec<StoredCertificate>,
}

impl Default for NetworkFile {
    fn default() -> Self {
        Self {
            version: FILE_VERSION,
            proxy: StoredProxy::default(),
            certificates: Vec::new(),
        }
    }
}

/// Reads `path`; a missing file is the defaults, a malformed one too (with a warning).
fn read_file(path: &Path) -> NetworkFile {
    let Ok(bytes) = std::fs::read(path) else {
        return NetworkFile::default();
    };
    let parsed = serde_json::from_slice::<NetworkFile>(&bytes)
        .ok()
        .filter(|file| file.version == FILE_VERSION && proxy::is_valid(&file.proxy))
        .filter(|file| file.certificates.len() <= MAX_CERTIFICATES)
        .filter(|file| file.certificates.iter().all(StoredCertificate::is_valid));
    parsed.unwrap_or_else(|| {
        tracing::warn!("network settings cannot be read; using the defaults");
        NetworkFile::default()
    })
}

/// The settings one send uses, fixed for its whole duration.
#[derive(Debug)]
pub struct NetConfig {
    pub generation: u32,
    proxy: StoredProxy,
    pub certificates: Vec<ClientCertificate>,
    password: Option<Zeroizing<String>>,
}

impl NetConfig {
    /// The proxy password of a `custom` proxy, for masking errors.
    pub fn password(&self) -> Option<&str> {
        self.password.as_ref().map(|password| password.as_str())
    }

    pub fn apply_proxy(
        &self,
        builder: reqwest::ClientBuilder,
    ) -> Result<reqwest::ClientBuilder, AppError> {
        proxy::apply(builder, &self.proxy, self.password())
    }

    /// Whether a hop to `url` goes through an `https://` proxy.
    pub fn https_proxy_for(&self, url: &url::Url) -> bool {
        proxy::is_https_for(&self.proxy, url)
    }
}

#[derive(Debug, Default)]
struct State {
    file: NetworkFile,
    generation: u32,
    current: Option<Arc<NetConfig>>,
}

pub struct Network {
    path: PathBuf,
    vault: Arc<dyn SecretStore>,
    state: Mutex<State>,
}

impl std::fmt::Debug for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Network").field("path", &self.path).finish()
    }
}

impl Network {
    /// Loads `<app_data>/network.json`. The vault is not opened here.
    pub fn load(app_data: &Path, vault: Arc<dyn SecretStore>) -> Self {
        let path = app_data.join(NETWORK_FILE);
        let file = read_file(&path);
        Self {
            path,
            vault,
            state: Mutex::new(State {
                file,
                ..State::default()
            }),
        }
    }

    pub fn vault(&self) -> Arc<dyn SecretStore> {
        Arc::clone(&self.vault)
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The proxy mode from `network.json`; never opens the vault.
    pub fn proxy_mode(&self) -> ProxyMode {
        self.lock().file.proxy.mode
    }

    /// The settings for a new send. Blocking: the first call may open the vault.
    pub fn current(&self) -> Result<Arc<NetConfig>, AppError> {
        let mut state = self.lock();
        if let Some(current) = &state.current {
            return Ok(Arc::clone(current));
        }
        let password = match state.file.proxy.mode {
            ProxyMode::Custom => password_of(self.vault.as_ref(), &state.file.proxy)?,
            ProxyMode::System | ProxyMode::None => None,
        };
        let config = Arc::new(NetConfig {
            generation: state.generation,
            proxy: state.file.proxy.clone(),
            certificates: state
                .file
                .certificates
                .iter()
                .map(|entry| entry.shown(false))
                .collect(),
            password,
        });
        state.current = Some(Arc::clone(&config));
        Ok(config)
    }

    /// What Network settings shows. Blocking (opens the vault).
    pub fn settings(&self) -> Result<NetworkSettings, AppError> {
        let file = self.lock().file.clone();
        self.shown(&file)
    }

    fn shown(&self, file: &NetworkFile) -> Result<NetworkSettings, AppError> {
        let vault = self.vault.as_ref();
        let has_password = password_of(vault, &file.proxy)?.is_some();
        let certificates = file
            .certificates
            .iter()
            .map(|entry| {
                let present = matches!(identity::load(vault, &entry.id), Ok(Some(_)));
                entry.shown(!present)
            })
            .collect();
        Ok(NetworkSettings {
            proxy: ProxySettings {
                mode: file.proxy.mode,
                url: file.proxy.url.clone(),
                username: file.proxy.username.clone(),
                has_password,
                bypass: file.proxy.bypass.clone(),
            },
            certificates,
            storage: vault.status(),
        })
    }

    /// Saves `next` and starts a new generation. The caller holds the lock.
    fn commit(&self, state: &mut State, next: NetworkFile) -> Result<(), AppError> {
        let bytes = serde_json::to_vec_pretty(&next).map_err(|source| {
            AppError::internal("Could not save the network settings.")
                .with_detail(source.to_string())
        })?;
        fsio::write_atomic(&self.path, &bytes)?;
        state.file = next;
        state.generation = state.generation.wrapping_add(1);
        state.current = None;
        Ok(())
    }

    /// Replaces the proxy; the password follows `SecretEdit`. Errors are masked with
    /// both the stored and the typed password.
    pub fn write_proxy(&self, input: &ProxyInput) -> Result<NetworkSettings, AppError> {
        let typed = match &input.password {
            SecretEdit::Set { value } => Some(Zeroizing::new(value.clone())),
            SecretEdit::Keep | SecretEdit::Clear => None,
        };
        let stored = self
            .vault
            .get(PROXY_PASSWORD_ID)
            .ok()
            .flatten()
            .map(Zeroizing::new);
        self.write_proxy_inner(input).map_err(|error| {
            let secrets: Vec<&str> = [&typed, &stored]
                .into_iter()
                .flatten()
                .map(|password| password.as_str())
                .collect();
            AppError {
                message: mask_secrets(&error.message, &secrets),
                detail: error.detail.map(|detail| mask_secrets(&detail, &secrets)),
                ..error
            }
        })
    }

    fn write_proxy_inner(&self, input: &ProxyInput) -> Result<NetworkSettings, AppError> {
        let proxy = proxy::validate(input)?;
        let mut state = self.lock();
        let changes = password_changes(
            self.vault.as_ref(),
            &input.password,
            &state.file.proxy,
            &proxy,
        )?;
        if !changes.is_empty() {
            self.vault.apply(&changes)?;
        }
        let next = NetworkFile {
            proxy,
            ..state.file.clone()
        };
        self.commit(&mut state, next.clone())?;
        drop(state);
        tracing::info!(mode = ?next.proxy.mode, host = %proxy::host_of(&next.proxy), "proxy settings saved");
        self.shown(&next)
    }

    /// Imports a certificate (and key) for `host`/`port`, replacing the entry for the
    /// same host and port.
    pub fn add_certificate(
        &self,
        host: &str,
        port: Option<u16>,
        cert_file: &Path,
        key_file: Option<&Path>,
    ) -> Result<NetworkSettings, AppError> {
        let host = identity::normalize_host(host)?;
        let bundle = identity::import(cert_file, key_file)?;
        let mut state = self.lock();
        let replaced: Vec<&StoredCertificate> = state
            .file
            .certificates
            .iter()
            .filter(|entry| entry.host == host && entry.port == port)
            .collect();
        if replaced.is_empty() && state.file.certificates.len() >= MAX_CERTIFICATES {
            return Err(AppError::invalid_input(TOO_MANY_CERTIFICATES));
        }
        let old_ids: Vec<String> = replaced.iter().map(|entry| entry.id.clone()).collect();
        let added = StoredCertificate {
            id: uuid::Uuid::new_v4().as_hyphenated().to_string(),
            host,
            port,
            cert_name: base_name(cert_file),
            key_name: key_file.map_or_else(|| base_name(cert_file), base_name),
            added_at_ms: now_ms(),
        };
        identity::store(self.vault.as_ref(), &added.id, &bundle)?;
        let certificates = state
            .file
            .certificates
            .iter()
            .filter(|entry| !old_ids.contains(&entry.id))
            .cloned()
            .chain([added])
            .collect();
        let next = NetworkFile {
            certificates,
            ..state.file.clone()
        };
        self.commit(&mut state, next.clone())?;
        drop(state);
        for id in &old_ids {
            identity::remove(self.vault.as_ref(), id)?;
        }
        self.shown(&next)
    }

    /// Removes a certificate listed in `network.json` and its vault value.
    pub fn remove_certificate(&self, id: &str) -> Result<NetworkSettings, AppError> {
        let mut state = self.lock();
        if !state.file.certificates.iter().any(|entry| entry.id == id) {
            return Err(AppError::not_found(UNKNOWN_CERTIFICATE));
        }
        identity::remove(self.vault.as_ref(), id)?;
        let next = NetworkFile {
            certificates: state
                .file
                .certificates
                .iter()
                .filter(|entry| entry.id != id)
                .cloned()
                .collect(),
            ..state.file.clone()
        };
        self.commit(&mut state, next.clone())?;
        drop(state);
        self.shown(&next)
    }
}

/// The file name only, for display (never the directory).
fn base_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "network_tests.rs"]
mod tests;
