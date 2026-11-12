//! Template resolution (plan 04 §5 rules). Total: an unresolved token is left as typed
//! and reported, never an error. Substituted text is never scanned again.
//!
//! Security rules (M4 review): a secret resolves only when the request's own text names
//! it, never from inside another variable's value (a hostile environment could otherwise
//! smuggle it to its own host), and its value is taken literally. A shared budget caps
//! the total expansion, so nested fan-out cannot blow up.

use std::collections::BTreeSet;

use super::dynamic::{Dynamics, DYNAMIC_NAMES};
use super::scopes::Scopes;
use super::template::{tokens, Segment};

/// Deepest chain of nested lookups that still resolves.
pub const MAX_DEPTH: usize = 10;
/// What a secret looks like in a preview.
pub const MASK: &str = "•••••";
/// Bytes of substituted text one resolution may produce; past it, tokens stay unresolved.
pub const MAX_EXPANDED_BYTES: usize = 1024 * 1024;
/// Variable lookups one resolution may make.
pub const MAX_LOOKUPS: usize = 10_000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub unresolved: BTreeSet<String>,
    /// Every secret value that was substituted, for redaction.
    pub secret_values: Vec<String>,
    pub used_secret: bool,
}

impl Report {
    pub fn secret_refs(&self) -> Vec<&str> {
        self.secret_values.iter().map(String::as_str).collect()
    }
}

/// Resolves `text` for sending: secrets and dynamic values are substituted.
pub fn resolve_text(
    text: &str,
    scopes: &Scopes,
    dynamics: &dyn Dynamics,
    report: &mut Report,
) -> String {
    let mut context = Context::new(scopes, Some(dynamics), report);
    context.expand(text, &mut Vec::new())
}

/// Resolves `text` for display: secrets (and values that use one) become [`MASK`],
/// dynamic variables are left as typed.
pub fn mask_text(text: &str, scopes: &Scopes, report: &mut Report) -> String {
    let mut context = Context::new(scopes, None, report);
    context.expand(text, &mut Vec::new())
}

enum Lookup {
    Value(String),
    AsTyped,
    Unresolved,
}

struct Context<'a> {
    scopes: &'a Scopes,
    /// `None` in mask mode.
    dynamics: Option<&'a dyn Dynamics>,
    report: &'a mut Report,
    lookups: usize,
    expanded_bytes: usize,
}

impl<'a> Context<'a> {
    fn new(scopes: &'a Scopes, dynamics: Option<&'a dyn Dynamics>, report: &'a mut Report) -> Self {
        Self {
            scopes,
            dynamics,
            report,
            lookups: 0,
            expanded_bytes: 0,
        }
    }

    fn over_budget(&mut self) -> bool {
        self.lookups += 1;
        self.lookups > MAX_LOOKUPS || self.expanded_bytes > MAX_EXPANDED_BYTES
    }

    fn expand(&mut self, text: &str, stack: &mut Vec<String>) -> String {
        let mut out = String::with_capacity(text.len().min(MAX_EXPANDED_BYTES));
        for segment in tokens(text) {
            match segment {
                Segment::Literal(literal) => out.push_str(literal),
                Segment::Escape => out.push_str("{{"),
                Segment::Var { name, range } => match self.lookup(name, stack) {
                    Lookup::Value(value) => {
                        self.expanded_bytes += value.len();
                        out.push_str(&value);
                    }
                    Lookup::AsTyped => out.push_str(&text[range]),
                    Lookup::Unresolved => {
                        self.report.unresolved.insert(name.to_string());
                        out.push_str(&text[range]);
                    }
                },
            }
        }
        out
    }

    fn lookup(&mut self, name: &str, stack: &mut Vec<String>) -> Lookup {
        if self.over_budget() {
            return Lookup::Unresolved;
        }
        if name.starts_with('$') {
            return self.dynamic(name);
        }
        if stack.len() >= MAX_DEPTH || stack.iter().any(|seen| seen == name) {
            return Lookup::Unresolved;
        }
        let Some(value) = self.scopes.lookup(name) else {
            return Lookup::Unresolved;
        };
        let Some(text) = value.text.clone() else {
            return Lookup::Unresolved;
        };
        if value.secret {
            return self.secret(text, stack.is_empty());
        }
        stack.push(name.to_string());
        let expanded = self.expand(&text, stack);
        stack.pop();
        Lookup::Value(expanded)
    }

    /// A secret: literal, and only where the request's own text names it.
    fn secret(&mut self, text: String, direct: bool) -> Lookup {
        if !direct {
            return Lookup::Unresolved;
        }
        self.report.used_secret = true;
        if !text.is_empty() && !self.report.secret_values.contains(&text) {
            self.report.secret_values.push(text.clone());
        }
        if self.dynamics.is_none() {
            Lookup::Value(MASK.to_string())
        } else {
            Lookup::Value(text)
        }
    }

    fn dynamic(&self, name: &str) -> Lookup {
        match self.dynamics {
            Some(dynamics) => dynamics
                .value(name)
                .map_or(Lookup::Unresolved, Lookup::Value),
            None if DYNAMIC_NAMES.contains(&name) => Lookup::AsTyped,
            None => Lookup::Unresolved,
        }
    }
}

#[cfg(test)]
#[path = "resolve_tests.rs"]
mod tests;
