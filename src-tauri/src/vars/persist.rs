//! Variable writes made by scripts (plan 05 §4.1), applied after a send.
//!
//! - Environment and collection: the local **current value** (never Git). A variable
//!   that does not exist yet is created with an empty shared value, so only its name
//!   reaches the YAML file.
//! - Globals: the globals file (already machine-local).
//! - A variable declared secret: the vault, never a plain file.
//! - A value equal to a known secret, written to a variable that is not secret: refused,
//!   so a script cannot copy a secret out of the vault into a plain file.
//! - A disabled variable is enabled again: the script's value would not resolve otherwise.
//! - Unset: removes the current value (globals: the variable). A variable whose shared
//!   value is empty is removed too, so a script-created one leaves no trace.
//!
//! Every write is applied independently; a refused one is reported, never fatal.

use super::local::Values;
use super::service::{ScopeData, VarsService};
use super::types::VarScope;
use super::vault::SecretChange;
use super::write::{save_current, save_table};
use crate::error::AppError;
use crate::store::env_model::{check_value, check_var_name, VarEntry, VarTable, MAX_VARIABLES};
use crate::store::Store;

const LABEL: &str = "Script";

/// One variable a script set (`Some`) or unset (`None`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptWrite {
    pub scope: VarScope,
    pub name: String,
    pub value: Option<String>,
}

/// One scope after the writes, before it is saved.
struct Planned {
    table: VarTable,
    current: Values,
    vault: Vec<SecretChange>,
}

impl VarsService {
    /// Applies `writes` scope by scope. `secrets` are the secret values the send knew, in
    /// any scope. Returns one message per write or scope that could not be saved;
    /// messages name variables, never values.
    pub fn apply_script_writes(
        &self,
        store: &mut Store,
        writes: &[ScriptWrite],
        secrets: &[String],
    ) -> Vec<String> {
        let mut scopes: Vec<&VarScope> = Vec::new();
        for write in writes {
            if !scopes.contains(&&write.scope) {
                scopes.push(&write.scope);
            }
        }
        scopes
            .into_iter()
            .flat_map(|scope| {
                let mine: Vec<&ScriptWrite> = writes.iter().filter(|w| &w.scope == scope).collect();
                self.apply_scope(store, scope, &mine, secrets)
                    .unwrap_or_else(|error| {
                        vec![format!("Could not save script variables: {error}")]
                    })
            })
            .collect()
    }

    fn apply_scope(
        &self,
        store: &mut Store,
        scope: &VarScope,
        writes: &[&ScriptWrite],
        secrets: &[String],
    ) -> Result<Vec<String>, AppError> {
        let old = self.load_scope(store, scope)?;
        let mut planned = Planned {
            table: old.table.clone(),
            current: old.current.clone(),
            vault: Vec::new(),
        };
        let mut refused = Vec::new();
        for write in writes {
            if leaks_secret(&planned.table, write, secrets) {
                refused.push(format!(
                    "`{}` was not saved: its value is a secret and `{0}` is not a secret variable.",
                    write.name
                ));
            } else if let Err(error) = plan_write(&old, scope, write, &mut planned) {
                refused.push(format!("Could not save `{}`: {error}", write.name));
            }
        }
        self.vault.apply(&planned.vault)?;
        if planned.current != old.current {
            save_current(store, scope, &planned.current)?;
        }
        if planned.table != old.table {
            save_table(store, scope, planned.table)?;
        }
        Ok(refused)
    }
}

/// Whether `write` would put a known secret value into a variable that is not secret.
fn leaks_secret(table: &VarTable, write: &ScriptWrite, secrets: &[String]) -> bool {
    let to_secret = table.get(&write.name).is_some_and(|entry| entry.secret);
    let is_secret = |value: &String| !value.is_empty() && secrets.contains(value);
    !to_secret && write.value.as_ref().is_some_and(is_secret)
}

fn plan_write(
    old: &ScopeData,
    scope: &VarScope,
    write: &ScriptWrite,
    planned: &mut Planned,
) -> Result<(), AppError> {
    let name = write.name.as_str();
    check_var_name(LABEL, name)?;
    if let Some(value) = &write.value {
        check_value(LABEL, name, value)?;
    }
    let is_global = matches!(scope, VarScope::Global);
    let existing = planned.table.get(name).cloned().map(|entry| VarEntry {
        // Unchanged unless the script sets a value below.
        enabled: entry.enabled || write.value.is_some(),
        ..entry
    });
    match (existing, &write.value) {
        (Some(entry), value) if entry.secret => {
            planned.table.insert(name.to_string(), entry);
            let id = old.secret_id(name);
            planned.vault.push(match value {
                Some(value) => SecretChange::Set {
                    id,
                    value: value.clone(),
                },
                None => SecretChange::Delete { id },
            });
        }
        (Some(entry), Some(value)) if is_global => {
            planned.table.insert(
                name.to_string(),
                VarEntry {
                    value: value.clone(),
                    ..entry
                },
            );
        }
        (Some(entry), Some(value)) => {
            planned.table.insert(name.to_string(), entry);
            planned.current.insert(name.to_string(), value.clone());
        }
        (Some(entry), None) => {
            planned.current.remove(name);
            if is_global || entry.value.is_empty() {
                planned.table.remove(name);
            }
        }
        (None, Some(value)) => {
            if planned.table.len() >= MAX_VARIABLES {
                return Err(AppError::invalid_input(
                    "A scope can have at most 500 variables.",
                ));
            }
            let shared = if is_global {
                value.clone()
            } else {
                String::new()
            };
            planned
                .table
                .insert(name.to_string(), VarEntry::new(shared, true, false));
            if !is_global {
                planned.current.insert(name.to_string(), value.clone());
            }
        }
        (None, None) => {}
    }
    Ok(())
}

#[cfg(test)]
#[path = "persist_tests.rs"]
mod tests;
