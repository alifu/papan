//! Runs a chain of scripts (collection → folders → request) against one context.
//!
//! The chain runs on its own OS thread with a known stack size, so the caller's thread
//! (a `spawn_blocking` worker) only waits, and never past the deadline plus a short grace
//! (see [`super::watchdog`]). The scripts work on a copy of the context that replaces the
//! caller's when the chain ends, whether it ended cleanly or with a script error; if the
//! engine itself fails, or the chain had to be abandoned, the caller's context is left as
//! it was.

use std::sync::Arc;
use std::time::Instant;

use super::context::{diff, ScriptContext};
use super::pm::{HostState, SendRequestBridge};
use super::report::{Redaction, Redactor, ScriptError, ScriptErrorKind, ScriptPhase, ScriptReport};
use super::sandbox::{stop_message, CancelFlag, Limits};
use super::watchdog::{watch, StrayThreads, Watched};

/// Largest script source accepted, in bytes (plan 05 §3).
pub const MAX_SCRIPT_BYTES: usize = 256 * 1024;

/// One script of a chain. `label` names it in errors (e.g. "collection", "request").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    pub label: String,
    pub source: String,
}

impl Script {
    pub fn new(label: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            source: source.into(),
        }
    }
}

/// Runs script chains. Cheap to build: make one per send.
#[derive(Clone, Default)]
pub struct ScriptEngine {
    bridge: Option<Arc<dyn SendRequestBridge>>,
    cancel: CancelFlag,
    redactor: Option<Redactor>,
    strays: StrayThreads,
}

impl ScriptEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes `pm.sendRequest` available, routed through `bridge`.
    #[must_use]
    pub fn with_bridge(self, bridge: Arc<dyn SendRequestBridge>) -> Self {
        Self {
            bridge: Some(bridge),
            ..self
        }
    }

    /// Stops a running chain when `cancel` is raised.
    #[must_use]
    pub fn with_cancel(self, cancel: CancelFlag) -> Self {
        Self { cancel, ..self }
    }

    /// Extra masking applied to every text of the report, after the context's own secrets.
    #[must_use]
    pub fn with_redactor(self, redactor: Redactor) -> Self {
        Self {
            redactor: Some(redactor),
            ..self
        }
    }

    /// Counts abandoned script threads together with every engine sharing `strays`.
    #[must_use]
    pub fn with_strays(self, strays: StrayThreads) -> Self {
        Self { strays, ..self }
    }

    pub fn cancel_flag(&self) -> CancelFlag {
        self.cancel.clone()
    }

    /// Runs `scripts` outer → inner in one fresh sandbox sharing `context`, and blocks
    /// until the chain ends. Never panics and never returns a secret in the report.
    pub fn run(
        &self,
        phase: ScriptPhase,
        scripts: &[Script],
        context: &mut ScriptContext,
        limits: &Limits,
    ) -> ScriptReport {
        let scripts: Vec<Script> = scripts
            .iter()
            .filter(|script| !script.source.trim().is_empty())
            .cloned()
            .collect();
        if let Some(script) = scripts.iter().find(|s| s.source.len() > MAX_SCRIPT_BYTES) {
            let message = format!("A script is larger than {MAX_SCRIPT_BYTES} bytes.");
            let report = ScriptReport::failed(phase, ScriptErrorKind::Exception, message);
            return with_script(report, &script.label);
        }
        if let Some(script) = scripts.iter().find(|s| s.source.contains('\0')) {
            let message = "A script cannot contain a NUL character.";
            let report = ScriptReport::failed(phase, ScriptErrorKind::Exception, message);
            return with_script(report, &script.label);
        }
        if scripts.is_empty() {
            return self.report(phase, context, context, HostOutput::default(), None);
        }
        if self.cancel.is_cancelled() {
            return ScriptReport::failed(
                phase,
                ScriptErrorKind::Cancelled,
                "The script was cancelled.",
            );
        }

        if self.strays.is_full() {
            return ScriptReport::failed(
                phase,
                ScriptErrorKind::Internal,
                "Too many stopped scripts are still running. Restart the app to run scripts.",
            );
        }

        let deadline = Instant::now() + limits.timeout;
        let host = HostState::new(
            phase,
            context.clone(),
            self.bridge.clone(),
            deadline,
            limits.max_send_requests,
        );
        let (host, error) = match watch(host, scripts, limits, &self.cancel, &self.strays) {
            Watched::Finished(host, error) => (*host, error),
            Watched::Abandoned(kind) => {
                return ScriptReport::failed(phase, kind, stop_message(kind))
            }
            Watched::Failed => return engine_failure(phase),
        };
        let output = HostOutput {
            console: host.console,
            console_truncated: host.console_truncated,
            tests: host.tests,
        };
        let report = self.report(phase, context, &host.ctx, output, error);
        *context = host.ctx;
        report
    }

    fn report(
        &self,
        phase: ScriptPhase,
        before: &ScriptContext,
        after: &ScriptContext,
        output: HostOutput,
        error: Option<ScriptError>,
    ) -> ScriptReport {
        let secrets = [before.secret_values(), after.secret_values()].concat();
        let redaction = Redaction::new(secrets, self.redactor.as_ref());
        ScriptReport {
            phase,
            console: output
                .console
                .iter()
                .map(|l| redaction.console(l))
                .collect(),
            console_truncated: output.console_truncated,
            tests: output.tests.iter().map(|t| redaction.test(t)).collect(),
            changes: diff(&before.layers, &after.layers)
                .iter()
                .map(|change| redaction.change(change))
                .collect(),
            error: error.as_ref().map(|e| redaction.error(e)),
        }
    }
}

#[derive(Default)]
struct HostOutput {
    console: Vec<super::report::ConsoleLine>,
    console_truncated: bool,
    tests: Vec<super::report::TestResult>,
}

fn engine_failure(phase: ScriptPhase) -> ScriptReport {
    ScriptReport::failed(
        phase,
        ScriptErrorKind::Internal,
        "The script engine failed unexpectedly.",
    )
}

fn with_script(report: ScriptReport, label: &str) -> ScriptReport {
    ScriptReport {
        error: report.error.map(|error| ScriptError {
            script: Some(label.to_string()),
            ..error
        }),
        ..report
    }
}

#[cfg(test)]
#[path = "chain_tests.rs"]
mod tests;
