//! Variables → Postman (plan 09 §3 secrets rule): initial values only; a secret's value is
//! empty unless it is passed in `secrets` (vault, on request). Environments and globals mark
//! it `type: "secret"`; the v2.1.0 collection schema only allows string/boolean/any/number, so
//! collection secrets are written as `type: "string"` and reported by the caller.

use super::SecretValues;
use crate::interop::postman::schema::{EnvValue, Variable, VariableFile};
use crate::store::env_model::{EnvFile, GlobalsFile, VarEntry, VarTable};

/// Key under which an imported Postman id is kept in our files' unknown fields.
pub(super) const POSTMAN_ID: &str = "postman_id";
const GLOBALS_NAME: &str = "Globals";

/// Collection variables (`variable[]`), sorted by name.
pub fn collection_variables(table: &VarTable, secrets: Option<&SecretValues>) -> Vec<Variable> {
    table
        .iter()
        .map(|(name, entry)| Variable {
            key: name.clone(),
            value: value_of(name, entry, secrets),
            kind: entry.secret.then(|| "string".to_string()),
            disabled: !entry.enabled,
            description: None,
        })
        .collect()
}

/// An environment file; its id is the imported `postman_id`, else `fallback_id`.
pub fn environment(
    env: &EnvFile,
    fallback_id: &str,
    secrets: Option<&SecretValues>,
) -> VariableFile {
    let id = env
        .extra
        .get(POSTMAN_ID)
        .and_then(|v| v.as_str())
        .unwrap_or(fallback_id)
        .to_string();
    VariableFile {
        id: Some(id),
        name: env.name.clone(),
        values: env_values(&env.variables, secrets),
        scope: Some("environment".into()),
    }
}

pub fn globals(file: &GlobalsFile, secrets: Option<&SecretValues>) -> VariableFile {
    VariableFile {
        id: None,
        name: GLOBALS_NAME.into(),
        values: env_values(&file.variables, secrets),
        scope: Some("globals".into()),
    }
}

fn env_values(table: &VarTable, secrets: Option<&SecretValues>) -> Vec<EnvValue> {
    table
        .iter()
        .map(|(name, entry)| EnvValue {
            key: name.clone(),
            value: value_of(name, entry, secrets),
            kind: Some(if entry.secret { "secret" } else { "default" }.into()),
            enabled: entry.enabled,
        })
        .collect()
}

/// A secret's value never comes from the file, only from `secrets`.
fn value_of(name: &str, entry: &VarEntry, secrets: Option<&SecretValues>) -> String {
    if entry.secret {
        secrets
            .and_then(|s| s.get(name))
            .cloned()
            .unwrap_or_default()
    } else {
        entry.value.clone()
    }
}
