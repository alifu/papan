//! Postman variables (collection `variable[]`, environment and globals `values[]`) to a
//! variable table (plan 09 §3 "Variables", §4 limits). Secret values never enter the
//! table: they are kept apart in [`SecretValues`].

use std::collections::BTreeSet;

use super::secrets::{looks_sensitive, SecretValues};
use crate::interop::postman::limits::{MAX_VALUE_BYTES, MAX_VARIABLES};
use crate::interop::postman::report::Report;
use crate::interop::postman::schema;
use crate::store::env_model::{check_var_name, VarEntry, VarTable};
use crate::vars::dynamic::DYNAMIC_NAMES;

const SECRET_TYPE: &str = "secret";

/// One scope's variables as imported.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlannedVars {
    pub table: VarTable,
    pub secrets: SecretValues,
    /// Non-secret variables whose names look sensitive (pre-ticked in the preview).
    pub candidates: Vec<String>,
}

/// A variable in either Postman form.
#[derive(Debug, Clone)]
pub struct RawVar<'a> {
    pub key: &'a str,
    pub value: &'a str,
    pub enabled: bool,
    pub secret: bool,
}

pub fn from_collection(vars: &[schema::Variable]) -> Vec<RawVar<'_>> {
    vars.iter()
        .map(|var| RawVar {
            key: &var.key,
            value: &var.value,
            enabled: !var.disabled,
            secret: var.kind.as_deref() == Some(SECRET_TYPE),
        })
        .collect()
}

pub fn from_values(values: &[schema::EnvValue]) -> Vec<RawVar<'_>> {
    values
        .iter()
        .map(|var| RawVar {
            key: &var.key,
            value: &var.value,
            enabled: var.enabled,
            secret: var.kind.as_deref() == Some(SECRET_TYPE),
        })
        .collect()
}

/// Builds a scope from `vars` at JSON path `path` (e.g. `values`), reporting skips.
pub fn plan_vars(file: &str, path: &str, vars: &[RawVar<'_>], report: &mut Report) -> PlannedVars {
    let mut planned = PlannedVars::default();
    let mut seen = BTreeSet::new();
    for (index, var) in vars.iter().enumerate() {
        let at = format!("{path}[{index}]");
        if let Err(error) = check_var_name(file, var.key) {
            report.skipped(file, &at, problem_of(&error.message, file));
            continue;
        }
        if !seen.insert(var.key) {
            report.skipped(
                file,
                &at,
                format!("A second variable `{}` was skipped.", var.key),
            );
            continue;
        }
        if planned.table.len() >= MAX_VARIABLES {
            report.skipped(
                file,
                &at,
                "A scope can have at most 500 variables; the rest were skipped.",
            );
            break;
        }
        if var.value.len() > MAX_VALUE_BYTES {
            report.skipped(
                file,
                &at,
                format!("The value of `{}` is longer than 64 KiB.", var.key),
            );
            continue;
        }
        report_dynamics(file, &at, var.value, report);
        if var.secret {
            planned.secrets.insert(var.key, var.value);
        } else if looks_sensitive(var.key) {
            planned.candidates.push(var.key.to_string());
        }
        let entry = VarEntry::new(var.value.to_string(), var.enabled, var.secret);
        planned.table.insert(var.key.to_string(), entry);
    }
    planned
}

/// `planned` with the variables named in `secret_names` (the user's choice) made secret:
/// their values move from the table into the secret values.
pub fn apply_choice(planned: &PlannedVars, secret_names: &[String]) -> PlannedVars {
    let mut next = planned.clone();
    for name in secret_names {
        let Some(entry) = planned.table.get(name) else {
            continue;
        };
        if entry.secret {
            continue;
        }
        next.secrets.insert(name, &entry.value);
        next.table.insert(
            name.clone(),
            VarEntry::new(String::new(), entry.enabled, true),
        );
    }
    next
}

fn problem_of(message: &str, file: &str) -> String {
    message
        .strip_prefix(file)
        .and_then(|rest| rest.strip_prefix(": "))
        .unwrap_or(message)
        .to_string()
}

/// Reports Postman dynamic variables (`{{$randomEmail}}` …) that we do not have.
pub fn report_dynamics(file: &str, path: &str, text: &str, report: &mut Report) {
    for name in unknown_dynamics(text) {
        report.warning(
            file,
            path,
            format!("The dynamic variable `{{{{{name}}}}}` does not exist in Papan."),
        );
    }
}

/// Distinct `{{$name}}` references in `text` that are not ours.
pub fn unknown_dynamics(text: &str) -> Vec<String> {
    let mut found = BTreeSet::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{$") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            break;
        };
        let name = after[..end].trim();
        let plain = name.len() <= 64 && name[1..].chars().all(|c| c.is_ascii_alphanumeric());
        if plain && !DYNAMIC_NAMES.contains(&name) {
            found.insert(name.to_string());
        }
        rest = &after[end..];
    }
    found.into_iter().collect()
}

#[cfg(test)]
#[path = "vars_tests.rs"]
mod tests;
