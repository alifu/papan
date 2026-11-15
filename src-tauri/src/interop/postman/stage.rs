//! Staged previews: at most 4, 10 minute TTL, UUID v4 tokens, holding the parsed plan so
//! files are not re-read on commit (plan 09 §6). Secret values inside a plan have a
//! redacting `Debug` and are dropped on commit, discard or expiry.

use std::time::{Duration, Instant};

use super::import::ImportPlan;
use super::report::Report;
use super::types::PostmanPreview;
use crate::error::AppError;

pub const MAX_STAGED: usize = 4;
pub const STAGE_TTL: Duration = Duration::from_secs(10 * 60);

pub struct Staged {
    token: String,
    plan: ImportPlan,
    created: Instant,
}

impl Staged {
    pub fn plan(&self) -> &ImportPlan {
        &self.plan
    }
}

/// Prints the token only: a plan can hold values of variables that merely look sensitive.
impl std::fmt::Debug for Staged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Staged")
            .field("token", &self.token)
            .field("plan", &"<hidden>")
            .finish()
    }
}

/// The staged plans, oldest first.
#[derive(Debug, Default)]
pub struct Stages {
    entries: Vec<Staged>,
}

impl Stages {
    /// Stages `plan` and returns the preview the UI shows. The oldest stage is dropped when
    /// the limit is reached.
    pub fn stage(&mut self, plan: ImportPlan, report: Report, now: Instant) -> PostmanPreview {
        self.expire(now);
        while self.entries.len() >= MAX_STAGED {
            self.entries.remove(0);
        }
        let token = uuid::Uuid::new_v4().to_string();
        let (items, truncated) = report.into_parts();
        let preview = PostmanPreview {
            token: token.clone(),
            files: plan.summaries.clone(),
            report: items,
            truncated,
        };
        self.entries.push(Staged {
            token,
            plan,
            created: now,
        });
        preview
    }

    /// A copy of the staged plan; the stage stays until committed, discarded or expired.
    pub fn get(&mut self, token: &str, now: Instant) -> Result<ImportPlan, AppError> {
        self.expire(now);
        self.entries
            .iter()
            .find(|staged| staged.token == token)
            .map(|staged| staged.plan.clone())
            .ok_or_else(|| {
                AppError::not_found("This import preview has expired. Pick the files again.")
            })
    }

    /// Takes a stage out so a second commit cannot run on it; [`Stages::put_back`] returns it
    /// (after an error or a cancelled dialog).
    pub fn take(&mut self, token: &str, now: Instant) -> Result<Staged, AppError> {
        self.expire(now);
        let at = self
            .entries
            .iter()
            .position(|staged| staged.token == token)
            .ok_or_else(|| {
                AppError::not_found("This import preview has expired. Pick the files again.")
            })?;
        Ok(self.entries.remove(at))
    }

    /// Returns a taken stage with its original age.
    pub fn put_back(&mut self, staged: Staged) {
        self.entries.insert(0, staged);
    }

    /// Drops one stage (unknown tokens are ignored) and every expired one.
    pub fn discard(&mut self, token: &str) {
        self.expire(Instant::now());
        self.entries.retain(|staged| staged.token != token);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn expire(&mut self, now: Instant) {
        self.entries
            .retain(|staged| now.saturating_duration_since(staged.created) < STAGE_TTL);
    }
}

#[cfg(test)]
#[path = "stage_tests.rs"]
mod tests;
