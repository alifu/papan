//! PKCE (RFC 7636, S256 only) and the `state` value. Pure apart from the random
//! source, which is injected.

use aws_lc_rs::rand::SecureRandom as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use zeroize::Zeroizing;

use crate::error::AppError;

/// Random bytes behind a verifier or a `state`: 256 bits.
pub const TOKEN_BYTES: usize = 32;

/// A source of cryptographically secure random bytes.
pub trait RandomBytes: Send + Sync {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), AppError>;
}

/// The system CSPRNG (`aws_lc_rs::rand::SystemRandom`).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRng;

impl RandomBytes for SystemRng {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), AppError> {
        aws_lc_rs::rand::SystemRandom::new()
            .fill(bytes)
            .map_err(|_| AppError::internal("Could not generate a random value."))
    }
}

/// 32 random bytes as base64url without padding (43 characters): a PKCE verifier or a
/// `state`.
pub fn random_token(rng: &dyn RandomBytes) -> Result<Zeroizing<String>, AppError> {
    let mut bytes = Zeroizing::new([0_u8; TOKEN_BYTES]);
    rng.fill(bytes.as_mut_slice())?;
    Ok(Zeroizing::new(URL_SAFE_NO_PAD.encode(bytes.as_slice())))
}

/// The S256 challenge of `verifier`: base64url(SHA-256(verifier)), no padding.
pub fn challenge(verifier: &str) -> String {
    let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest.as_ref())
}

/// Whether `left` equals `right`, in time that does not depend on where they differ.
pub fn same(left: &str, right: &str) -> bool {
    aws_lc_rs::constant_time::verify_slices_are_equal(left.as_bytes(), right.as_bytes()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fills with one repeated byte.
    struct Fixed(u8);

    impl RandomBytes for Fixed {
        fn fill(&self, bytes: &mut [u8]) -> Result<(), AppError> {
            bytes.fill(self.0);
            Ok(())
        }
    }

    struct Broken;

    impl RandomBytes for Broken {
        fn fill(&self, _: &mut [u8]) -> Result<(), AppError> {
            Err(AppError::internal("no entropy"))
        }
    }

    #[test]
    fn challenge_matches_the_rfc_7636_appendix_b_vector() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";

        let challenge = challenge(verifier);

        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn random_token_is_32_bytes_as_unpadded_base64url() {
        let token = random_token(&Fixed(0xff)).unwrap();

        assert_eq!(token.len(), 43);
        assert_eq!(
            URL_SAFE_NO_PAD.decode(token.as_bytes()).unwrap(),
            [0xff; 32]
        );
        assert!(token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn random_token_fails_when_the_random_source_fails() {
        let result = random_token(&Broken);

        assert!(result.is_err());
    }

    #[test]
    fn system_rng_gives_a_different_token_each_time() {
        let first = random_token(&SystemRng).unwrap();
        let second = random_token(&SystemRng).unwrap();

        assert_ne!(*first, *second);
        assert_eq!(first.len(), 43);
    }

    #[test]
    fn same_accepts_equal_text_and_refuses_any_difference() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "abcd"));
        assert!(!same("", "a"));
        assert!(same("", ""));
    }
}
