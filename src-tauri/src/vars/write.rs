//! `write_variables` (plan 04 §7): the whole scope is replaced. Writes are ordered so a
//! crash leaves extra data, never missing data: vault sets → local file → YAML → vault
//! deletes. A secret value is never revealed into a plain variable.

use std::collections::{BTreeMap, BTreeSet};

use super::local;
use super::service::{ScopeData, VarsService};
use super::types::{SecretEdit, VarScope, Variable, VariableInput};
use super::vault::SecretChange;
use crate::error::AppError;
use crate::store::env_model::{
    check_value, check_var_name, VarEntry, VarTable, MAX_VALUE_BYTES, MAX_VARIABLES,
};
use crate::store::Store;

const INPUT_LABEL: &str = "Variables";

fn validate(inputs: &[VariableInput]) -> Result<(), AppError> {
    if inputs.len() > MAX_VARIABLES {
        return Err(AppError::invalid_input(
            "A scope can have at most 500 variables.",
        ));
    }
    let mut seen = BTreeSet::new();
    for input in inputs {
        check_var_name(INPUT_LABEL, &input.name)?;
        check_value(INPUT_LABEL, &input.name, &input.value)?;
        let current = input.current.as_deref().unwrap_or_default();
        let secret = match &input.secret_edit {
            SecretEdit::Set { value } => value.as_str(),
            SecretEdit::Keep | SecretEdit::Clear => "",
        };
        if current.len() > MAX_VALUE_BYTES || secret.len() > MAX_VALUE_BYTES {
            return Err(AppError::invalid_input(format!(
                "The value of `{}` is longer than 64 KiB.",
                input.name
            )));
        }
        if !seen.insert(input.name.as_str()) {
            return Err(AppError::invalid_input(format!(
                "Two variables are named `{}`.",
                input.name
            )));
        }
    }
    Ok(())
}

/// What the write does to the vault: sets first, deletes last.
#[derive(Debug, Default)]
struct VaultPlan {
    sets: Vec<SecretChange>,
    kept: BTreeSet<String>,
    deletes: BTreeSet<String>,
}

impl VarsService {
    pub fn write_variables(
        &self,
        store: &mut Store,
        scope: &VarScope,
        inputs: &[VariableInput],
    ) -> Result<Vec<Variable>, AppError> {
        validate(inputs)?;
        let old = self.load_scope(store, scope)?;
        let plan = self.plan_vault(&old, inputs)?;
        let table = new_table(&old, inputs);
        let current = new_current(scope, &table, inputs);

        self.vault.apply(&plan.sets)?;
        save_current(store, scope, &current)?;
        save_table(store, scope, table.clone())?;
        let deletes: Vec<SecretChange> = plan
            .deletes
            .difference(&plan.kept)
            .map(|id| SecretChange::Delete { id: id.clone() })
            .collect();
        self.vault.apply(&deletes)?;

        self.variables_of(&ScopeData {
            table,
            current,
            ..old
        })
    }

    fn plan_vault(&self, old: &ScopeData, inputs: &[VariableInput]) -> Result<VaultPlan, AppError> {
        let mut plan = VaultPlan {
            deletes: old
                .table
                .iter()
                .filter(|(_, entry)| entry.secret)
                .map(|(name, _)| old.secret_id(name))
                .collect(),
            ..VaultPlan::default()
        };
        for input in inputs.iter().filter(|input| input.secret) {
            let id = old.secret_id(&input.name);
            let previous_name = old_name(old, input);
            let previous = previous_name.and_then(|name| old.table.get(name).map(|e| (name, e)));
            let value = match (&input.secret_edit, previous) {
                (SecretEdit::Set { value }, _) => Some(value.clone()),
                (SecretEdit::Clear, _) => {
                    plan.deletes.insert(id.clone());
                    None
                }
                (SecretEdit::Keep, Some((name, entry))) if entry.secret => {
                    if name == input.name {
                        plan.kept.insert(id.clone());
                        None
                    } else {
                        self.vault.get(&old.secret_id(name))?
                    }
                }
                (SecretEdit::Keep, Some((name, entry))) => {
                    let effective = old.current.get(name).unwrap_or(&entry.value);
                    (!effective.is_empty()).then(|| effective.clone())
                }
                (SecretEdit::Keep, None) => None,
            };
            if let Some(value) = value {
                plan.kept.insert(id.clone());
                plan.sets.push(SecretChange::Set { id, value });
            }
        }
        Ok(plan)
    }
}

/// The name a row had before this write, if it existed.
fn old_name<'a>(old: &'a ScopeData, input: &'a VariableInput) -> Option<&'a str> {
    let name = input.previous_name.as_deref().unwrap_or(&input.name);
    old.table.contains_key(name).then_some(name)
}

fn new_table(old: &ScopeData, inputs: &[VariableInput]) -> VarTable {
    inputs
        .iter()
        .map(|input| {
            let extra = old_name(old, input)
                .map(|name| old.table[name].extra.clone())
                .unwrap_or_default();
            let entry = VarEntry {
                extra,
                ..VarEntry::new(input.value.clone(), input.enabled, input.secret)
            };
            (input.name.clone(), entry)
        })
        .collect()
}

/// Local current values of plain variables; none for globals or secrets.
fn new_current(scope: &VarScope, table: &VarTable, inputs: &[VariableInput]) -> local::Values {
    if matches!(scope, VarScope::Global) {
        return BTreeMap::new();
    }
    inputs
        .iter()
        .filter(|input| !table[&input.name].secret)
        .filter_map(|input| Some((input.name.clone(), input.current.clone()?)))
        .collect()
}

fn save_current(store: &Store, scope: &VarScope, current: &local::Values) -> Result<(), AppError> {
    let (collection_id, env) = match scope {
        VarScope::Global => return Ok(()),
        VarScope::Collection { collection_id } => (collection_id, None),
        VarScope::Environment { collection_id, env } => (collection_id, Some(env)),
    };
    let mut state = local::load(store.app_data(), collection_id)?;
    match env {
        None => state.current.collection = current.clone(),
        Some(env) if current.is_empty() => {
            state.current.environments.remove(env);
        }
        Some(env) => {
            state
                .current
                .environments
                .insert(env.clone(), current.clone());
        }
    }
    local::save(store.app_data(), collection_id, &state)
}

fn save_table(store: &mut Store, scope: &VarScope, table: VarTable) -> Result<(), AppError> {
    match scope {
        VarScope::Global => store.write_globals(table),
        VarScope::Collection { collection_id } => {
            store.write_collection_variables(collection_id, table)
        }
        VarScope::Environment { collection_id, env } => {
            store.write_environment_variables(collection_id, env, table)
        }
    }
}
