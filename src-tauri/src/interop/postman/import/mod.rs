//! Postman import (plan 09 §9: Opus-A): picked files → [`ImportPlan`] + report, all in
//! memory; [`write`] puts a plan on disk. Import never sends, never runs scripts and never
//! follows a path found in a file.

pub mod auth;
pub mod body;
pub mod detect;
pub mod events;
pub mod request;
pub mod secrets;
pub mod tree;
pub mod vars;
pub mod vault;
pub mod write;

mod options;
mod rename;
mod render;

pub use options::with_options;

use self::tree::{plan_collection, PlannedCollection};
use self::vars::{from_values, plan_vars, PlannedVars};
use super::limits::{self, clean_display_name, file_error, MAX_ENVIRONMENTS};
use super::report::Report;
use super::schema::{self, SchemaError};
use super::types::{PostmanFileKind, PostmanFileSummary};
use crate::error::AppError;

const UNTITLED_ENVIRONMENT: &str = "Environment";

/// One environment file, ready to write.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedEnv {
    pub source: String,
    pub name: String,
    pub vars: PlannedVars,
}

/// Everything an import will write, validated. Secret values are in [`SecretValues`]
/// (redacting `Debug`) and only reach the vault when the user opts in.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImportPlan {
    pub collection: Option<PlannedCollection>,
    pub environments: Vec<PlannedEnv>,
    pub globals: Option<PlannedVars>,
    pub summaries: Vec<PostmanFileSummary>,
}

impl ImportPlan {
    /// Total scripts in the collection tree (for the "switched off" headline).
    pub fn scripts(&self) -> u32 {
        self.collection.as_ref().map_or(0, |c| c.scripts)
    }
}

/// Parses, checks and maps the picked files (`(file name, bytes)`).
pub fn plan_files(files: &[(String, Vec<u8>)]) -> Result<(ImportPlan, Report), AppError> {
    limits::check_file_count(files.len())?;
    let mut report = Report::new();
    let mut plan = ImportPlan::default();
    for (file, bytes) in files {
        let value = limits::parse_json(file, bytes)?;
        let kind = detect::detect(file, &value)?;
        match kind {
            PostmanFileKind::CollectionV20 | PostmanFileKind::CollectionV21 => {
                add_collection(&mut plan, file, kind, &value, &mut report)?;
            }
            PostmanFileKind::Environment | PostmanFileKind::Globals => {
                add_variable_file(&mut plan, file, kind, &value, &mut report)?;
            }
        }
    }
    let report = with_headline(plan.scripts(), report);
    Ok((plan, report))
}

fn with_headline(scripts: u32, report: Report) -> Report {
    if scripts == 0 {
        return report;
    }
    let mut headline = Report::new();
    headline.warning(
        "",
        "",
        format!(
            "{scripts} scripts imported but switched off; enable them per script after reading them."
        ),
    );
    headline.extend(report);
    headline
}

fn schema_error(file: &str, error: &SchemaError) -> AppError {
    file_error(file, &error.to_string())
}

fn note_unknown(file: &str, unknown: u32, report: &mut Report) {
    if unknown > 0 {
        report.info(
            file,
            "",
            format!("{unknown} unknown field(s) were ignored."),
        );
    }
}

fn add_collection(
    plan: &mut ImportPlan,
    file: &str,
    kind: PostmanFileKind,
    value: &serde_json::Value,
    report: &mut Report,
) -> Result<(), AppError> {
    if plan.collection.is_some() {
        return Err(file_error(
            file,
            "Import one collection at a time (with its environments).",
        ));
    }
    let parsed = schema::parse_collection(value).map_err(|e| schema_error(file, &e))?;
    note_unknown(file, parsed.unknown_fields, report);
    let planned = plan_collection(file, &parsed.value, report)?;
    plan.summaries.push(PostmanFileSummary {
        file: file.to_string(),
        kind,
        name: planned.name.clone(),
        requests: planned.requests,
        folders: planned.folders,
        variables: count(planned.vars.table.len()),
        scripts: planned.scripts,
        secret_candidates: planned.vars.candidates.clone(),
    });
    plan.collection = Some(planned);
    Ok(())
}

fn add_variable_file(
    plan: &mut ImportPlan,
    file: &str,
    kind: PostmanFileKind,
    value: &serde_json::Value,
    report: &mut Report,
) -> Result<(), AppError> {
    let parsed = schema::parse_variable_file(value).map_err(|e| schema_error(file, &e))?;
    note_unknown(file, parsed.unknown_fields, report);
    let (name, _) = clean_display_name(&parsed.value.name, UNTITLED_ENVIRONMENT);
    let vars = plan_vars(file, "values", &from_values(&parsed.value.values), report);
    plan.summaries.push(PostmanFileSummary {
        file: file.to_string(),
        kind,
        name: name.clone(),
        requests: 0,
        folders: 0,
        variables: count(vars.table.len()),
        scripts: 0,
        secret_candidates: vars.candidates.clone(),
    });
    if kind == PostmanFileKind::Globals {
        plan.globals = Some(merge_globals(plan.globals.take(), vars, file, report));
        return Ok(());
    }
    if plan.environments.len() >= MAX_ENVIRONMENTS {
        report.skipped(
            file,
            "",
            "A collection can have at most 50 environments; this one was skipped.",
        );
        return Ok(());
    }
    plan.environments.push(PlannedEnv {
        source: file.to_string(),
        name,
        vars,
    });
    Ok(())
}

/// Several globals files: the first file's variable wins on a name clash.
fn merge_globals(
    current: Option<PlannedVars>,
    next: PlannedVars,
    file: &str,
    report: &mut Report,
) -> PlannedVars {
    let Some(mut merged) = current else {
        return next;
    };
    for (name, entry) in next.table {
        if merged.table.contains_key(&name) {
            report.skipped(
                file,
                "values",
                format!("The global `{name}` is already in another file."),
            );
            continue;
        }
        if let Some(value) = next.secrets.get(&name) {
            merged.secrets.insert(&name, value);
        }
        if next.candidates.contains(&name) {
            merged.candidates.push(name.clone());
        }
        merged.table.insert(name, entry);
    }
    merged
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
