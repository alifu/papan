//! Imported secret values into the vault, after the files are in place and the collection
//! is registered (plan 09 §3 rule 1, §4). Failures are reported, never fatal.

use super::secrets::SecretValues;
use super::write::PendingSecrets;
use crate::error::AppError;
use crate::interop::postman::report::Report;
use crate::store::Store;
use crate::vars::types::{SecretEdit, VarScope, VariableInput};
use crate::vars::VarsService;

/// Stores pending secret values with `write_variables` (call after the collection is
/// registered as `collection_id`). Failures are reported; files stay as written.
pub fn apply_secrets(
    vars: &VarsService,
    store: &mut Store,
    collection_id: &str,
    pending: &PendingSecrets,
) -> Report {
    let mut report = Report::new();
    let collection = VarScope::Collection {
        collection_id: collection_id.to_string(),
    };
    let envs = pending.environments.iter().map(|(env, values)| {
        let scope = VarScope::Environment {
            collection_id: collection_id.to_string(),
            env: env.clone(),
        };
        (scope, values)
    });
    let scopes = std::iter::once((collection, &pending.collection))
        .chain(envs)
        .chain(std::iter::once((VarScope::Global, &pending.globals)));
    for (scope, values) in scopes.filter(|(_, values)| !values.is_empty()) {
        if let Err(error) = store_scope(vars, store, &scope, values) {
            tracing::warn!(code = ?error.code, "storing imported secret values failed");
            report.warning(
                "",
                "",
                format!("Secret values could not be stored: {}", error.message),
            );
        }
    }
    report
}

fn store_scope(
    vars: &VarsService,
    store: &mut Store,
    scope: &VarScope,
    values: &SecretValues,
) -> Result<(), AppError> {
    let inputs: Vec<VariableInput> = vars
        .read_variables(store, scope)?
        .into_iter()
        .map(|variable| {
            let set = variable
                .secret
                .then(|| values.get(&variable.name))
                .flatten();
            VariableInput {
                secret_edit: set.map_or(SecretEdit::Keep, |value| SecretEdit::Set {
                    value: value.to_string(),
                }),
                name: variable.name,
                previous_name: None,
                value: variable.value,
                current: variable.current,
                enabled: variable.enabled,
                secret: variable.secret,
            }
        })
        .collect();
    vars.write_variables(store, scope, &inputs).map(|_| ())
}
