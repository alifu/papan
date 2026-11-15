//! Import flow without Tauri (plan 09 §7): read picked files, plan, and commit a staged plan
//! into a new or an open collection. The commands only add dialogs, locks and events.

use std::path::{Path, PathBuf};

use super::import::vault::apply_secrets;
use super::import::write::{write_into_collection, write_new_collection, Written};
use super::import::{plan_files, with_options, ImportPlan};
use super::limits::{self, file_label};
use super::report::{Report, MAX_REPORT_ITEMS};
use super::types::{ImportOptions, ImportTarget, PostmanImportResult, ReportItem, ReportLevel};
use crate::error::AppError;
use crate::store::Store;
use crate::vars::VarsService;

/// Reads the picked files (≤ 20, regular files only, ≤ 5 MiB each) and plans the import.
pub fn plan_paths(paths: &[PathBuf]) -> Result<(ImportPlan, Report), AppError> {
    limits::check_file_count(paths.len())?;
    let files = paths
        .iter()
        .map(|path| Ok((file_label(path), limits::read_file(path)?)))
        .collect::<Result<Vec<_>, AppError>>()?;
    plan_files(&files)
}

/// What a commit needs before any dialog opens: a collection to create or to import into.
pub fn check_target(plan: &ImportPlan, options: &ImportOptions) -> Result<(), AppError> {
    match &options.target {
        ImportTarget::NewCollection if plan.collection.is_none() => Err(AppError::invalid_input(
            "Environments and globals need an open collection to import into.",
        )),
        _ => Ok(()),
    }
}

/// A new collection in `parent`, opened in the store.
pub fn commit_new(
    plan: &ImportPlan,
    options: &ImportOptions,
    parent: &Path,
    store: &mut Store,
    vars: &VarsService,
) -> Result<PostmanImportResult, AppError> {
    check_target(plan, options)?;
    let chosen = with_options(plan, options);
    let written = write_new_collection(&chosen, options, parent, store)?;
    let tree = match store.open_collection(&written.root) {
        Ok(tree) => tree,
        Err(error) => {
            // Nothing partial: the folder that was just made goes away again.
            if let Err(cleanup) = crate::store::fsio::remove_dir_all(&written.root) {
                tracing::warn!(detail = ?cleanup.detail, "could not remove an unopened import");
            }
            return Err(error);
        }
    };
    Ok(finish(&chosen, written, &tree.collection.id, store, vars))
}

/// A new top-level folder (plus environments and globals) in the open collection `target`.
pub fn commit_into(
    plan: &ImportPlan,
    options: &ImportOptions,
    store: &mut Store,
    vars: &VarsService,
) -> Result<PostmanImportResult, AppError> {
    let ImportTarget::Collection { collection_id } = &options.target else {
        return Err(AppError::invalid_input("Pick a collection to import into."));
    };
    let chosen = with_options(plan, options);
    let written = write_into_collection(&chosen, options, store, collection_id)?;
    Ok(finish(&chosen, written, collection_id, store, vars))
}

fn finish(
    plan: &ImportPlan,
    written: Written,
    collection_id: &str,
    store: &mut Store,
    vars: &VarsService,
) -> PostmanImportResult {
    let mut report = Report::new();
    let scripts = plan.scripts();
    if scripts > 0 {
        report.warning(
            "",
            "",
            format!(
                "{scripts} scripts imported but switched off; enable them per script after reading them."
            ),
        );
    }
    report.extend(written.report);
    report.extend(apply_secrets(vars, store, collection_id, &written.pending));
    PostmanImportResult {
        collection_id: plan.collection.as_ref().map(|_| collection_id.to_string()),
        environments: written.environments,
        globals: written.globals,
        report: capped(report),
    }
}

/// Report entries as the UI gets them: at most 1000, the rest as one closing note.
pub fn capped(report: Report) -> Vec<ReportItem> {
    let (mut items, truncated) = report.into_parts();
    note_truncated(&mut items, truncated);
    items
}

/// Appends "and N more" when entries were cut (also used for exports).
pub fn note_truncated(items: &mut Vec<ReportItem>, extra: u32) {
    if extra > 0 {
        items.push(ReportItem {
            level: ReportLevel::Info,
            file: String::new(),
            path: String::new(),
            message: format!("and {extra} more."),
        });
    }
}

/// Caps an already collected list at the report limit.
pub fn cap_items(mut items: Vec<ReportItem>) -> Vec<ReportItem> {
    if items.len() > MAX_REPORT_ITEMS {
        let extra = u32::try_from(items.len() - MAX_REPORT_ITEMS).unwrap_or(u32::MAX);
        items.truncate(MAX_REPORT_ITEMS);
        note_truncated(&mut items, extra);
    }
    items
}
