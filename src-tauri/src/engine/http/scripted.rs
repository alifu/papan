//! `send_request` with scripts (plan 05 §4.2):
//!
//! collect scripts → pre-request chain → a new draft from `pm.request` (the editor's
//! draft is never changed) → resolve `{{vars}}` with the chain's changes and a runtime
//! layer on top → send → post-response chain (body from the kept response file, capped)
//! → persist variable changes → history with the original draft.
//!
//! A pre-request error stops the send before anything goes on the wire; the outcome then
//! has no `meta` but still carries the report. A post-response error keeps the response.
//! A transport failure skips the post-response chain. When a chain already ran, the
//! outcome has no `meta`, carries the error in `error` and keeps the reports; otherwise it
//! is a plain `Err`. A cancel is always an `Err`. A script error keeps the variable
//! changes made before it. Cancel stops a running chain.
//! The store lock is released before any JavaScript runs.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use super::inflight::cancelled;
use super::mask::mask_secrets;
use super::resolve::resolve_content;
use super::responses::validate_id;
use super::script_bridge::{read_capped, ChainRunner};
use super::script_map::{
    collect_chains, content_after, layers_of, scopes_after, script_request, script_response,
    secrets_of, with_secrets, writes_of, Chains,
};
use super::service::record_history;
use super::types::SendOutcome;
use super::variables::now_ms;
use super::HttpEngine;
use crate::error::{AppError, ErrorCode};
use crate::script::{
    ConsoleLevel, ConsoleLine, Limits, Redactor, ScriptContext, ScriptErrorKind, ScriptPhase,
    ScriptReport,
};
use crate::store::types::{RequestContent, RequestLocation};
use crate::store::StoreState;
use crate::vars::dynamic::Dynamics;
use crate::vars::scopes::Scopes;
use crate::vars::VarsService;

/// What a send reads from disk and the vault before any script runs.
struct Prepared {
    scopes: Scopes,
    /// The selected environment, where `pm.environment` changes are saved.
    env: Option<String>,
    chains: Chains,
}

/// The services one send works with.
pub struct SendDeps<'a> {
    pub engine: &'a HttpEngine,
    pub store: &'a StoreState,
    pub vars: &'a VarsService,
    pub dynamics: Arc<dyn Dynamics>,
}

/// Runs the whole pipeline for the draft `content` at `location`.
pub async fn run_send(
    deps: &SendDeps<'_>,
    request_id: &str,
    location: &RequestLocation,
    content: &RequestContent,
) -> Result<SendOutcome, AppError> {
    run_send_with(deps, request_id, location, content, &BTreeMap::new()).await
}

/// [`run_send`] with `overlay` variables on the runtime layer: what a journey passes from
/// one node to the next (plan 13 §3). They win over the environment and scripts may change them.
pub async fn run_send_with(
    deps: &SendDeps<'_>,
    request_id: &str,
    location: &RequestLocation,
    content: &RequestContent,
    overlay: &BTreeMap<String, String>,
) -> Result<SendOutcome, AppError> {
    let sent_at_ms = now_ms();
    let started = Instant::now();
    let id = validate_id(request_id)?;
    let run = deps.engine.script_run(&id);
    let prepared = prepare(deps, location, content).await?;
    let runner = ChainRunner {
        engine: deps.engine,
        location,
        draft: content,
        cancel: run.flag.clone(),
        redactor: redactor_of(&prepared.scopes),
        secrets: secrets_of(&prepared.scopes),
        strays: run.strays.clone(),
        limits: Limits::default(),
    };
    let mut reports = Vec::new();
    let mut layers = layers_of(&prepared.scopes);
    layers.runtime.values.extend(overlay.clone());
    let mut context = ScriptContext::new(layers, script_request(content));

    if prepared.chains.has_pre() {
        let scripts = prepared.chains.pre_request.clone();
        let (after, report) = runner.run(ScriptPhase::PreRequest, scripts, context).await;
        context = after;
        let stopped = report.error.as_ref().map(|error| error.kind);
        reports.push(report);
        if let Some(kind) = stopped {
            let scripts = persist(deps, location, &prepared, &context, reports).await;
            if kind == ScriptErrorKind::Cancelled {
                return Err(cancelled());
            }
            return Ok(SendOutcome {
                meta: None,
                scripts,
                error: None,
            });
        }
    }

    let draft = content_after(content, &context.request);
    let scopes = scopes_after(&prepared.scopes, &context);
    // A script may have put a secret anywhere in the request, so mask them all.
    let resolved = with_secrets(
        resolve_content(&draft, &scopes, deps.dynamics.as_ref()),
        &secrets_of(&scopes),
    );
    let result = deps.engine.send_resolved(&id, location, &resolved).await;
    context.extra_secrets = resolved.secret_values.clone();

    if let (Ok(meta), true) = (&result, prepared.chains.has_post()) {
        let body = match deps.engine.body_file(&id) {
            Ok(path) => read_capped(&path).await,
            Err(_) => Vec::new(),
        };
        context.response = Some(script_response(meta, &body));
        let scripts = prepared.chains.post_response.clone();
        let (after, report) = runner
            .run(ScriptPhase::PostResponse, scripts, context)
            .await;
        context = after;
        reports.push(report);
    }

    let scripts = persist(deps, location, &prepared, &context, reports).await;
    record_history(
        deps.store,
        location,
        content,
        sent_at_ms,
        started.elapsed(),
        &result,
    );
    match result {
        Ok(meta) => Ok(SendOutcome {
            meta: Some(meta),
            scripts,
            error: None,
        }),
        // Keep the pre-request report next to a transport failure; a cancel stays an `Err`.
        Err(error) if !scripts.is_empty() && error.code != ErrorCode::Cancelled => {
            Ok(SendOutcome {
                meta: None,
                scripts,
                error: Some(error),
            })
        }
        Err(error) => Err(error),
    }
}

/// Scopes, the selected environment and the scripts, off the async runtime: the store
/// lock is released before the vault is read (it may prompt).
async fn prepare(
    deps: &SendDeps<'_>,
    location: &RequestLocation,
    content: &RequestContent,
) -> Result<Prepared, AppError> {
    let store = Arc::clone(&deps.store.store);
    let vars = deps.vars.clone();
    let location = location.clone();
    let request_scripts = content.scripts.clone();
    tokio::task::spawn_blocking(move || {
        let (snapshot, env, chains) = {
            let guard = store
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let env = match &location {
                RequestLocation::Collection { collection_id, .. } => vars
                    .list_environments(&guard, collection_id)
                    .ok()
                    .and_then(|list| list.selected),
                RequestLocation::Scratch { .. } => None,
            };
            let chains = collect_chains(&guard, &location, &request_scripts)?;
            (vars.snapshot(&guard, &location), env, chains)
        };
        Ok(Prepared {
            scopes: vars.scopes(&snapshot),
            env,
            chains,
        })
    })
    .await
    .map_err(|source| {
        AppError::internal("Could not prepare the request.").with_detail(source.to_string())
    })?
}

/// Masks every secret of `scopes`, also percent-encoded, in another case, or in base64.
fn redactor_of(scopes: &Scopes) -> Redactor {
    let secrets = secrets_of(scopes);
    Arc::new(move |text: &str| {
        let refs: Vec<&str> = secrets.iter().map(String::as_str).collect();
        mask_secrets(text, &refs)
    })
}

/// Saves the chains' variable changes; anything that could not be saved is added to the
/// last report as a warning.
async fn persist(
    deps: &SendDeps<'_>,
    location: &RequestLocation,
    prepared: &Prepared,
    context: &ScriptContext,
    reports: Vec<ScriptReport>,
) -> Vec<ScriptReport> {
    let (writes, dropped) = writes_of(context, location, prepared.env.as_deref());
    let refused = if writes.is_empty() {
        Vec::new()
    } else {
        let store = Arc::clone(&deps.store.store);
        let vars = deps.vars.clone();
        let secrets = context.secret_values();
        tokio::task::spawn_blocking(move || {
            let mut guard = store
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            vars.apply_script_writes(&mut guard, &writes, &secrets)
        })
        .await
        .unwrap_or_else(|error| vec![format!("Could not save script variables: {error}")])
    };
    with_warnings(reports, dropped.into_iter().chain(refused).collect())
}

fn with_warnings(reports: Vec<ScriptReport>, warnings: Vec<String>) -> Vec<ScriptReport> {
    if warnings.is_empty() {
        return reports;
    }
    for warning in &warnings {
        tracing::warn!("{warning}");
    }
    let count = reports.len();
    reports
        .into_iter()
        .enumerate()
        .map(|(index, report)| {
            if index + 1 != count {
                return report;
            }
            let lines = warnings.iter().map(|text| ConsoleLine {
                level: ConsoleLevel::Warn,
                text: text.clone(),
            });
            ScriptReport {
                console: report.console.into_iter().chain(lines).collect(),
                ..report
            }
        })
        .collect()
}
