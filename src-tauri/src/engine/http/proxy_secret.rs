//! The proxy password in the vault (plan 03b §4.1, §15.2), bound to the proxy's
//! `host:port`: it is only sent to the host it was saved for.

use zeroize::Zeroizing;

use super::proxy::{self, StoredProxy};
use crate::error::AppError;
use crate::vars::types::SecretEdit;
use crate::vars::vault::{SecretChange, SecretStore};

pub const PROXY_PASSWORD_ID: &str = "n/proxy/password";
/// The `host:port` the proxy password was saved for.
pub const PROXY_ENDPOINT_ID: &str = "n/proxy/endpoint";

const EMPTY_PASSWORD: &str = "Enter the proxy password, or clear it.";

/// The stored password, only while `proxy` still points at the `host:port` it was saved
/// for: an edited `network.json` never sends it to another host.
pub fn password_of(
    vault: &dyn SecretStore,
    proxy: &StoredProxy,
) -> Result<Option<Zeroizing<String>>, AppError> {
    let endpoint = proxy::endpoint_of(proxy);
    if endpoint.is_empty() || vault.get(PROXY_ENDPOINT_ID)?.as_deref() != Some(&endpoint) {
        return Ok(None);
    }
    Ok(vault.get(PROXY_PASSWORD_ID)?.map(Zeroizing::new))
}

/// The vault changes a proxy write makes: `Keep` holds only for the same `host:port`.
pub fn password_changes(
    vault: &dyn SecretStore,
    edit: &SecretEdit,
    old: &StoredProxy,
    next: &StoredProxy,
) -> Result<Vec<SecretChange>, AppError> {
    let delete = |id: &str| SecretChange::Delete { id: id.to_string() };
    let set = |id: &str, value: String| SecretChange::Set {
        id: id.to_string(),
        value,
    };
    let unbound = proxy::endpoint_of(old) != proxy::endpoint_of(next);
    Ok(match edit {
        SecretEdit::Set { value } if value.is_empty() => {
            return Err(AppError::invalid_input(EMPTY_PASSWORD));
        }
        SecretEdit::Set { value } => vec![
            set(PROXY_PASSWORD_ID, value.clone()),
            set(PROXY_ENDPOINT_ID, proxy::endpoint_of(next)),
        ],
        SecretEdit::Keep if unbound && vault.has(PROXY_PASSWORD_ID)? => {
            vec![delete(PROXY_PASSWORD_ID), delete(PROXY_ENDPOINT_ID)]
        }
        SecretEdit::Keep => Vec::new(),
        SecretEdit::Clear => vec![delete(PROXY_PASSWORD_ID), delete(PROXY_ENDPOINT_ID)],
    })
}
