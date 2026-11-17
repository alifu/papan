//! `VarsService`: environments, variable reads, the scopes of a request and previews
//! (plan 04 §5, §7). Writes live in `write.rs`. Callers hold the store lock; the vault
//! is behind `SecretStore`. Building scopes is split in two so the store lock can be
//! released before the vault is touched (it may prompt).

use std::sync::Arc;

use super::dynamic::DYNAMIC_NAMES;
use super::local::{self, Values};
use super::resolve::{mask_text, Report};
use super::scopes::{Layer, Scopes, Value};
use super::types::{
    EffectiveVariable, EnvironmentList, SecretStorageMode, SecretStorageStatus, TemplatePreview,
    VarScope, VarSource, Variable,
};
use super::vault::SecretStore;
use crate::error::AppError;
use crate::store::env_model::{VarTable, MAX_VALUE_BYTES};
use crate::store::types::RequestLocation;
use crate::store::Store;

/// One scope's variables as stored, before the vault is consulted.
#[derive(Debug, Clone, PartialEq)]
pub struct ScopeData {
    pub source: VarSource,
    pub table: VarTable,
    /// Local current values (empty for globals).
    pub current: Values,
    /// Prefix of this scope's vault ids (plan 04 §4.1).
    pub prefix: String,
}

impl ScopeData {
    pub fn secret_id(&self, name: &str) -> String {
        format!("{}{name}", self.prefix)
    }
}

/// The scopes of a request location, read from disk only.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub scopes: Vec<ScopeData>,
}

pub fn global_prefix() -> String {
    "g/".to_string()
}

pub fn collection_prefix(collection_id: &str) -> String {
    format!("c/{collection_id}/")
}

pub fn env_prefix(collection_id: &str, env: &str) -> String {
    format!("e/{collection_id}/{env}/")
}

#[derive(Clone)]
pub struct VarsService {
    pub(super) vault: Arc<dyn SecretStore>,
}

impl std::fmt::Debug for VarsService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("VarsService")
    }
}

impl VarsService {
    pub fn new(vault: Arc<dyn SecretStore>) -> Self {
        Self { vault }
    }

    pub fn list_environments(
        &self,
        store: &Store,
        collection_id: &str,
    ) -> Result<EnvironmentList, AppError> {
        let environments = store.environments(collection_id)?;
        let state = local::load(store.app_data(), collection_id)?;
        let selected = state
            .selected_env
            .filter(|env| environments.iter().any(|e| &e.env == env && !e.invalid));
        Ok(EnvironmentList {
            environments,
            selected,
        })
    }

    pub fn create_environment(
        &self,
        store: &mut Store,
        collection_id: &str,
        name: &str,
    ) -> Result<EnvironmentList, AppError> {
        store.create_environment(collection_id, name)?;
        self.list_environments(store, collection_id)
    }

    pub fn rename_environment(
        &self,
        store: &mut Store,
        collection_id: &str,
        env: &str,
        name: &str,
    ) -> Result<EnvironmentList, AppError> {
        store.rename_environment(collection_id, env, name)?;
        self.list_environments(store, collection_id)
    }

    /// Deletes the file, then its local values and secrets.
    pub fn delete_environment(
        &self,
        store: &mut Store,
        collection_id: &str,
        env: &str,
    ) -> Result<EnvironmentList, AppError> {
        store.delete_environment(collection_id, env)?;
        let mut state = local::load(store.app_data(), collection_id)?;
        state.current.environments.remove(env);
        if state.selected_env.as_deref() == Some(env) {
            state.selected_env = None;
        }
        local::save(store.app_data(), collection_id, &state)?;
        self.vault.delete_prefix(&env_prefix(collection_id, env))?;
        self.list_environments(store, collection_id)
    }

    pub fn select_environment(
        &self,
        store: &Store,
        collection_id: &str,
        env: Option<&str>,
    ) -> Result<(), AppError> {
        let listed = store.environments(collection_id)?;
        if let Some(env) = env {
            let usable = listed.iter().any(|e| e.env == env && !e.invalid);
            if !usable {
                return Err(AppError::invalid_input(
                    "That environment cannot be selected.",
                ));
            }
        }
        let mut state = local::load(store.app_data(), collection_id)?;
        state.selected_env = env.map(str::to_string);
        local::save(store.app_data(), collection_id, &state)
    }

    /// Reads one scope as stored.
    pub fn load_scope(&self, store: &Store, scope: &VarScope) -> Result<ScopeData, AppError> {
        match scope {
            VarScope::Global => Ok(ScopeData {
                source: VarSource::Global,
                table: store.globals()?.variables,
                current: Values::new(),
                prefix: global_prefix(),
            }),
            VarScope::Collection { collection_id } => Ok(ScopeData {
                source: VarSource::Collection,
                table: store.collection_variables(collection_id)?,
                current: local::load(store.app_data(), collection_id)?
                    .current
                    .collection,
                prefix: collection_prefix(collection_id),
            }),
            VarScope::Environment { collection_id, env } => {
                let table = store.read_environment(collection_id, env)?.variables;
                let mut state = local::load(store.app_data(), collection_id)?;
                Ok(ScopeData {
                    source: VarSource::Environment,
                    table,
                    current: state.current.environments.remove(env).unwrap_or_default(),
                    prefix: env_prefix(collection_id, env),
                })
            }
        }
    }

    /// Variables of `scope`, sorted by name. Secret values are never returned.
    pub fn read_variables(
        &self,
        store: &Store,
        scope: &VarScope,
    ) -> Result<Vec<Variable>, AppError> {
        let data = self.load_scope(store, scope)?;
        self.variables_of(&data)
    }

    pub(super) fn variables_of(&self, data: &ScopeData) -> Result<Vec<Variable>, AppError> {
        data.table
            .iter()
            .map(|(name, entry)| {
                let has_secret = entry.secret && self.vault.has(&data.secret_id(name))?;
                let current = (!entry.secret)
                    .then(|| data.current.get(name).cloned())
                    .flatten();
                Ok(Variable {
                    name: name.clone(),
                    value: entry.value.clone(),
                    current,
                    enabled: entry.enabled,
                    secret: entry.secret,
                    has_secret,
                })
            })
            .collect()
    }

    /// The scopes that apply at `location`, from disk only. A scope that cannot be read
    /// is skipped (logged), so a broken file never blocks sending.
    pub fn snapshot(&self, store: &Store, location: &RequestLocation) -> Snapshot {
        let mut scopes = vec![VarScope::Global];
        if let RequestLocation::Collection { collection_id, .. } = location {
            scopes.push(VarScope::Collection {
                collection_id: collection_id.clone(),
            });
            let selected = self
                .list_environments(store, collection_id)
                .ok()
                .and_then(|list| list.selected);
            scopes.extend(selected.map(|env| VarScope::Environment {
                collection_id: collection_id.clone(),
                env,
            }));
        }
        let loaded = scopes
            .iter()
            .filter_map(|scope| {
                self.load_scope(store, scope)
                    .map_err(|error| tracing::warn!(detail = ?error.detail, "{error}"))
                    .ok()
            })
            .collect();
        Snapshot { scopes: loaded }
    }

    /// Resolver layers for `snapshot`; secrets are read from the vault here.
    pub fn scopes(&self, snapshot: &Snapshot) -> Scopes {
        let layers = snapshot
            .scopes
            .iter()
            .map(|data| Layer {
                source: data.source,
                vars: data
                    .table
                    .iter()
                    .filter(|(_, entry)| entry.enabled)
                    .map(|(name, entry)| (name.clone(), self.value_of(data, name, entry.secret)))
                    .collect(),
            })
            .collect();
        Scopes::new(layers)
    }

    fn value_of(&self, data: &ScopeData, name: &str, secret: bool) -> Value {
        if !secret {
            let entry = &data.table[name];
            let text = data.current.get(name).unwrap_or(&entry.value);
            return Value::plain(text.clone());
        }
        let stored = self
            .vault
            .get(&data.secret_id(name))
            .unwrap_or_else(|error| {
                tracing::warn!(detail = ?error.detail, "{error}");
                None
            });
        Value::secret(stored)
    }

    pub fn status(&self) -> SecretStorageStatus {
        self.vault.status()
    }

    /// The storage mode after the vault has been opened (a lookup opens it lazily).
    pub fn probe_status(&self) -> SecretStorageStatus {
        if let Err(error) = self.vault.has(&global_prefix()) {
            tracing::warn!(detail = ?error.detail, "{error}");
        }
        self.vault.status()
    }

    /// Deletes the vault file and key; refused unless the vault is damaged, so a
    /// healthy vault can never be wiped from the webview.
    pub fn reset(&self) -> Result<SecretStorageStatus, AppError> {
        if self.probe_status().mode != SecretStorageMode::Damaged {
            return Err(AppError::invalid_input(
                "Secret storage can only be reset when it is damaged.",
            ));
        }
        self.vault.reset()?;
        Ok(self.vault.status())
    }
}

/// Every defined variable after layering, then the dynamic names. No secret value.
pub fn effective_variables(scopes: &Scopes) -> Vec<EffectiveVariable> {
    let defined = scopes.names().into_iter().filter_map(|name| {
        let (value, source) = scopes.lookup_with_source(name)?;
        let mut report = Report::default();
        let shown = mask_text(&format!("{{{{{name}}}}}"), scopes, &mut report);
        Some(EffectiveVariable {
            name: name.to_string(),
            value: if value.secret { String::new() } else { shown },
            source,
            secret: value.secret,
            resolved: report.unresolved.is_empty(),
        })
    });
    let dynamic = DYNAMIC_NAMES.iter().map(|name| EffectiveVariable {
        name: (*name).to_string(),
        value: String::new(),
        source: VarSource::Dynamic,
        secret: false,
        resolved: true,
    });
    defined.chain(dynamic).collect()
}

/// `text` resolved for display (plan 04 §7, command 35).
pub fn preview(scopes: &Scopes, text: &str) -> Result<TemplatePreview, AppError> {
    if text.len() > MAX_VALUE_BYTES {
        return Err(AppError::invalid_input(
            "The text to preview is longer than 64 KiB.",
        ));
    }
    let mut report = Report::default();
    let shown = mask_text(text, scopes, &mut report);
    Ok(TemplatePreview {
        text: shown,
        unresolved: report.unresolved.into_iter().collect(),
    })
}
