//! The values one OAuth flow keeps out of every error it returns (plan 03b §15.2).

use zeroize::Zeroizing;

use super::super::mask::mask_secrets;
use super::super::resolve::Resolved;
use super::token::Issued;
use crate::error::AppError;
use crate::store::types::Auth;

/// An error message of a flow is cut to this many characters, after masking.
pub const MAX_ERROR_CHARS: usize = 500;

#[derive(Default)]
pub(super) struct MaskSet(Vec<Zeroizing<String>>);

impl MaskSet {
    /// The secrets a draft brings: every secret variable value it used and its client
    /// secret, typed or resolved.
    pub(super) fn of(resolved: &Resolved) -> Self {
        let mut mask = Self::default();
        for secret in &resolved.secret_values {
            mask.add(secret);
        }
        if let Auth::Oauth2 { client_secret, .. } = &resolved.content.auth {
            mask.add(client_secret);
        }
        mask
    }

    pub(super) fn issued(&mut self, issued: &Issued) {
        self.add(&issued.access_token);
        if let Some(refresh_token) = &issued.refresh_token {
            self.add(refresh_token);
        }
    }

    pub(super) fn add(&mut self, secret: &str) {
        if !secret.is_empty() {
            self.0.push(Zeroizing::new(secret.to_string()));
        }
    }

    /// `error` with every secret masked; the message is cut to [`MAX_ERROR_CHARS`]
    /// only afterwards, so a cut never leaves part of a secret behind.
    pub(super) fn mask(&self, error: AppError) -> AppError {
        let secrets: Vec<&str> = self.0.iter().map(|secret| secret.as_str()).collect();
        AppError {
            message: mask_secrets(&error.message, &secrets)
                .chars()
                .take(MAX_ERROR_CHARS)
                .collect(),
            detail: error
                .detail
                .as_deref()
                .map(|detail| mask_secrets(detail, &secrets)),
            ..error
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mask_set_covers_message_and_detail() {
        let mut mask = MaskSet::default();
        mask.add("");
        mask.add("hunter2-secret");

        let masked = mask.mask(
            AppError::invalid_input("bad hunter2-secret here").with_detail("also hunter2%2Dsecret"),
        );

        assert!(!masked.message.contains("hunter2"));
        assert!(!masked.detail.unwrap().contains("hunter2"));
    }

    #[test]
    fn a_secret_across_the_cut_is_masked_before_the_message_is_cut() {
        let mut mask = MaskSet::default();
        mask.add("hunter2-secret");
        let text = format!("{}hunter2-secret tail", "x".repeat(MAX_ERROR_CHARS - 4));

        let masked = mask.mask(AppError::invalid_input(text));

        assert_eq!(masked.message.chars().count(), MAX_ERROR_CHARS);
        assert!(!masked.message.contains("hunt"));
    }
}
