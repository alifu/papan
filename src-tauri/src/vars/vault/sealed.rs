//! Sealed files that are not the vault (plan 03b §6.5, §15.5): the vault's AES-256-GCM
//! format under another 8-byte magic, and its owner-only atomic write. The magic is part
//! of the authenticated header, so a sealed file never opens as a file of another kind.

use std::path::Path;

use zeroize::Zeroizing;

use super::{crypto, file_vault};
use crate::error::AppError;

pub use super::crypto::{generate_key, CryptoError, KEY_LEN, OVERHEAD};

/// Encrypts `plaintext` into a complete file that starts with `magic`.
pub fn seal(magic: &[u8; 8], key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    crypto::seal_with(magic, key, plaintext)
}

/// Decrypts a file written by `seal` under the same `magic` and key.
pub fn open(magic: &[u8; 8], key: &[u8], file: &[u8]) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    crypto::open_with(magic, key, file)
}

/// Replaces `target` atomically; the file is readable by its owner only.
pub fn write_private(target: &Path, bytes: &[u8]) -> Result<(), AppError> {
    file_vault::write_private(target, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const JAR: &[u8; 8] = b"PAPANJAR";
    const VAULT: &[u8; 8] = b"PAPANVLT";
    const KEY: [u8; KEY_LEN] = [7; KEY_LEN];

    #[test]
    fn a_sealed_file_starts_with_its_magic_and_opens_under_it() {
        let file = seal(JAR, &KEY, b"cookies").unwrap();

        assert_eq!(&file[..8], JAR);
        assert_eq!(file.len(), b"cookies".len() + OVERHEAD);
        assert_eq!(open(JAR, &KEY, &file).unwrap().as_slice(), b"cookies");
    }

    #[test]
    fn the_vault_magic_gives_the_vault_format() {
        let by_vault = crypto::seal(&KEY, b"entries").unwrap();
        let by_magic = seal(VAULT, &KEY, b"entries").unwrap();

        assert_eq!(open(VAULT, &KEY, &by_vault).unwrap().as_slice(), b"entries");
        assert_eq!(
            crypto::open(&KEY, &by_magic).unwrap().as_slice(),
            b"entries"
        );
    }

    #[test]
    fn a_file_of_one_kind_does_not_open_as_another_even_with_its_magic_rewritten() {
        let file = seal(JAR, &KEY, b"cookies").unwrap();
        let mut relabelled = file.clone();
        relabelled[..8].copy_from_slice(VAULT);

        assert_eq!(
            open(VAULT, &KEY, &file).unwrap_err(),
            CryptoError::BadFormat
        );
        assert_eq!(
            crypto::open(&KEY, &relabelled).unwrap_err(),
            CryptoError::Tampered
        );
    }
}
