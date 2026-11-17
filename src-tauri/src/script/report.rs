//! What a script chain produced, in the shape the webview receives.
//!
//! Nothing in a [`ScriptReport`] may carry a secret: every text goes through
//! [`Redaction`] and a secret variable's change is listed by name only.

use std::sync::Arc;

use serde::Serialize;
use specta::Type;

use super::context::{Scope, VarChange};
use super::pm::{MAX_CONSOLE_LINE_BYTES, MAX_TEST_TEXT_BYTES};

const MASK: &str = "[REDACTED]";
/// Ends a text that was cut.
const CUT_MARK: char = '…';
/// An encoded secret is at most this many times longer than the secret (`\u00XX`).
const LONGEST_ENCODING: usize = 6;

/// `text` cut to at most `max` bytes on a character boundary, with a marker when cut.
pub(super) fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let end = (0..=max)
        .rev()
        .find(|i| text.is_char_boundary(*i))
        .unwrap_or(0);
    format!("{}{CUT_MARK}", &text[..end])
}

/// `text` as it appears inside a JSON string: what `JSON.stringify` prints of a secret.
pub fn json_escaped(text: &str) -> String {
    let quoted = serde_json::to_string(text).unwrap_or_default();
    let inner = quoted.strip_prefix('"').and_then(|q| q.strip_suffix('"'));
    inner.unwrap_or(text).to_string()
}

/// Which chain ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ScriptPhase {
    PreRequest,
    PostResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ConsoleLevel {
    Log,
    Info,
    Warn,
    Error,
}

/// One `console.*` call, or a warning from the host (level `warn`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ConsoleLine {
    pub level: ConsoleLevel,
    pub text: String,
}

/// One `pm.test(name, fn)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ScriptScope {
    Globals,
    Collection,
    Environment,
    Runtime,
}

impl From<Scope> for ScriptScope {
    fn from(scope: Scope) -> Self {
        match scope {
            Scope::Globals => Self::Globals,
            Scope::Collection => Self::Collection,
            Scope::Environment => Self::Environment,
            Scope::Runtime => Self::Runtime,
        }
    }
}

/// A variable the chain changed. `value` is `None` for an unset and for a secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ScriptVarChange {
    pub scope: ScriptScope,
    pub name: String,
    pub value: Option<String>,
    pub secret: bool,
    pub removed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ScriptErrorKind {
    /// The script threw, or did not parse.
    Exception,
    /// The chain ran past its wall-clock limit.
    Timeout,
    /// The chain used more memory than allowed.
    Memory,
    /// Recursion went deeper than the stack limit.
    StackOverflow,
    /// The user cancelled the send.
    Cancelled,
    /// The engine itself failed.
    Internal,
}

/// Why a chain stopped early.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ScriptError {
    pub kind: ScriptErrorKind,
    pub message: String,
    /// Label of the script that failed (e.g. "collection", "request").
    pub script: Option<String>,
    /// 1-based line in that script, when known.
    pub line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ScriptReport {
    pub phase: ScriptPhase,
    pub console: Vec<ConsoleLine>,
    /// `console` lost lines to the output limit.
    pub console_truncated: bool,
    pub tests: Vec<TestResult>,
    pub changes: Vec<ScriptVarChange>,
    pub error: Option<ScriptError>,
}

impl ScriptReport {
    /// A report for a chain that could not run at all.
    pub fn failed(phase: ScriptPhase, kind: ScriptErrorKind, message: impl Into<String>) -> Self {
        Self {
            phase,
            console: Vec::new(),
            console_truncated: false,
            tests: Vec::new(),
            changes: Vec::new(),
            error: Some(ScriptError {
                kind,
                message: message.into(),
                script: None,
                line: None,
            }),
        }
    }
}

/// Extra masking supplied by the caller (e.g. `Resolved::redact`, which also knows the
/// percent-encoded forms of a secret).
pub type Redactor = Arc<dyn Fn(&str) -> String + Send + Sync>;

/// Masks secret values in text bound for the webview.
pub struct Redaction<'a> {
    /// Each secret and its JSON-escaped form; longest first, so a secret containing
    /// another is masked whole.
    secrets: Vec<String>,
    extra: Option<&'a Redactor>,
}

impl<'a> Redaction<'a> {
    pub fn new(secrets: Vec<String>, extra: Option<&'a Redactor>) -> Self {
        let mut secrets: Vec<String> = secrets
            .into_iter()
            .filter(|s| !s.is_empty())
            .flat_map(|secret| [json_escaped(&secret), secret])
            .collect();
        secrets.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        secrets.dedup();
        Self { secrets, extra }
    }

    /// `text` masked, then cut to `max` bytes: never the other way round, which would
    /// leave the start of a secret at the cut. A text the host already cut may end in
    /// such a start, so its tail goes too.
    pub fn clipped(&self, text: &str, max: usize) -> String {
        let masked = self.text(text);
        if !text.ends_with(CUT_MARK) {
            return truncate(&masked, max);
        }
        let longest = self.secrets.first().map_or(0, String::len);
        let keep = masked
            .len()
            .saturating_sub(CUT_MARK.len_utf8() + longest * LONGEST_ENCODING);
        let end = (0..=keep)
            .rev()
            .find(|i| masked.is_char_boundary(*i))
            .unwrap_or(0);
        truncate(&format!("{}{CUT_MARK}", &masked[..end]), max)
    }

    pub fn text(&self, text: &str) -> String {
        let masked = self
            .secrets
            .iter()
            .fold(text.to_string(), |acc, secret| acc.replace(secret, MASK));
        match self.extra {
            Some(redact) => redact(&masked),
            None => masked,
        }
    }

    pub fn console(&self, line: &ConsoleLine) -> ConsoleLine {
        ConsoleLine {
            level: line.level,
            text: self.clipped(&line.text, MAX_CONSOLE_LINE_BYTES),
        }
    }

    pub fn test(&self, test: &TestResult) -> TestResult {
        TestResult {
            name: self.clipped(&test.name, MAX_TEST_TEXT_BYTES),
            passed: test.passed,
            error: test
                .error
                .as_deref()
                .map(|error| self.clipped(error, MAX_TEST_TEXT_BYTES)),
        }
    }

    pub fn error(&self, error: &ScriptError) -> ScriptError {
        ScriptError {
            message: self.text(&error.message),
            ..error.clone()
        }
    }

    /// A secret variable is listed by name only; other values are masked like any text.
    pub fn change(&self, change: &VarChange) -> ScriptVarChange {
        ScriptVarChange {
            scope: change.scope.into(),
            name: self.text(&change.name),
            value: match (&change.value, change.secret) {
                (Some(value), false) => Some(self.text(value)),
                _ => None,
            },
            secret: change.secret,
            removed: change.value.is_none(),
        }
    }
}

#[cfg(test)]
#[path = "report_tests.rs"]
mod tests;
