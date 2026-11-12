//! Variable and environment types shared with the frontend through the IPC bindings
//! (plan 04 §7). No type here ever carries a secret value.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Which set of variables a read or write addresses.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum VarScope {
    Global,
    Collection { collection_id: String },
    Environment { collection_id: String, env: String },
}

/// One environment file of a collection; `env` is its file stem (its identity).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EnvironmentSummary {
    pub env: String,
    pub name: String,
    /// The file failed to parse: it can only be deleted.
    pub invalid: bool,
}

/// A collection's environments, sorted by name then `env`, and the selected one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EnvironmentList {
    pub environments: Vec<EnvironmentSummary>,
    pub selected: Option<String>,
}

/// A variable as the editor shows it. For a secret, `value` is "" and `current` None.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Variable {
    pub name: String,
    pub value: String,
    pub current: Option<String>,
    pub enabled: bool,
    pub secret: bool,
    /// A secret value is stored on this machine.
    pub has_secret: bool,
}

/// What to do with a variable's stored secret value on write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SecretEdit {
    Keep,
    Set { value: String },
    Clear,
}

/// A variable row sent by the editor; `previous_name` is set on a rename.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct VariableInput {
    pub name: String,
    pub previous_name: Option<String>,
    pub value: String,
    pub current: Option<String>,
    pub enabled: bool,
    pub secret: bool,
    pub secret_edit: SecretEdit,
}

/// Where an effective variable's value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum VarSource {
    Global,
    Collection,
    Environment,
    Dynamic,
}

/// A variable after layering. `value` is "" when secret; dynamic names are listed too.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EffectiveVariable {
    pub name: String,
    pub value: String,
    pub source: VarSource,
    pub secret: bool,
    pub resolved: bool,
}

/// A template resolved for display: secrets as "•••••", dynamics left as typed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TemplatePreview {
    pub text: String,
    pub unresolved: Vec<String>,
}

/// How secret values are kept on this machine (plan 04 §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SecretStorageMode {
    Keychain,
    SessionOnly,
    Damaged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SecretStorageStatus {
    pub mode: SecretStorageMode,
    pub message: Option<String>,
}
