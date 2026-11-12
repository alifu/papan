//! Sealing and opening the vault file (plan 04 §4.1). Pure: no I/O, no global state.
//!
//! Layout: `"PAPANVLT"` (8 bytes) + version `0x01` + 12-byte nonce + AES-256-GCM
//! ciphertext and tag. The 9 header bytes are the additional authenticated data.

use aws_lc_rs::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use aws_lc_rs::rand::{SecureRandom, SystemRandom};
use zeroize::Zeroizing;

pub const KEY_LEN: usize = 32;

const MAGIC: &[u8; 8] = b"PAPANVLT";
const VERSION: u8 = 0x01;
const HEADER_LEN: usize = MAGIC.len() + 1;
const TAG_LEN: usize = 16;
/// Bytes a sealed file adds to its plaintext.
pub const OVERHEAD: usize = HEADER_LEN + NONCE_LEN + TAG_LEN;

/// Why sealing or opening failed. Carries no key or plaintext material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CryptoError {
    /// The key is not 32 bytes.
    BadKey,
    /// The bytes are not a vault file of a known version.
    BadFormat,
    /// Authentication failed: wrong key, or the file was changed.
    Tampered,
    /// The system random generator or the cipher failed.
    Failed,
}

fn header() -> [u8; HEADER_LEN] {
    let mut header = [0u8; HEADER_LEN];
    header[..MAGIC.len()].copy_from_slice(MAGIC);
    header[MAGIC.len()] = VERSION;
    header
}

fn cipher(key: &[u8]) -> Result<LessSafeKey, CryptoError> {
    if key.len() != KEY_LEN {
        return Err(CryptoError::BadKey);
    }
    UnboundKey::new(&AES_256_GCM, key)
        .map(LessSafeKey::new)
        .map_err(|_| CryptoError::BadKey)
}

/// A new random 256-bit vault key.
pub fn generate_key() -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    let mut key = Zeroizing::new(vec![0u8; KEY_LEN]);
    SystemRandom::new()
        .fill(&mut key)
        .map_err(|_| CryptoError::Failed)?;
    Ok(key)
}

/// Encrypts `plaintext` into a complete vault file, under a fresh random nonce.
pub fn seal(key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let cipher = cipher(key)?;
    let header = header();
    let mut nonce = [0u8; NONCE_LEN];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| CryptoError::Failed)?;

    // Encrypted in place inside the output buffer, so the plaintext is not left behind
    // in a second allocation.
    let mut file = Vec::with_capacity(plaintext.len() + OVERHEAD);
    file.extend_from_slice(&header);
    file.extend_from_slice(&nonce);
    file.extend_from_slice(plaintext);
    let body_start = HEADER_LEN + NONCE_LEN;
    let sealed = cipher.seal_in_place_separate_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(header),
        &mut file[body_start..],
    );
    match sealed {
        Ok(tag) => {
            file.extend_from_slice(tag.as_ref());
            Ok(file)
        }
        Err(_) => {
            zeroize::Zeroize::zeroize(&mut file);
            Err(CryptoError::Failed)
        }
    }
}

/// Decrypts a complete vault file. Any change to header, nonce, ciphertext or tag fails.
pub fn open(key: &[u8], file: &[u8]) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    let cipher = cipher(key)?;
    let header = header();
    if file.len() < OVERHEAD || file[..HEADER_LEN] != header {
        return Err(CryptoError::BadFormat);
    }
    let body_start = HEADER_LEN + NONCE_LEN;
    let nonce = Nonce::try_assume_unique_for_key(&file[HEADER_LEN..body_start])
        .map_err(|_| CryptoError::BadFormat)?;
    let mut buffer = Zeroizing::new(file[body_start..].to_vec());
    let plain_len = cipher
        .open_in_place(nonce, Aad::from(header), &mut buffer)
        .map_err(|_| CryptoError::Tampered)?
        .len();
    buffer.truncate(plain_len);
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAINTEXT: &[u8] = br#"{"version":1,"entries":{"g/token":"s3cr3t-value"}}"#;

    fn key(fill: u8) -> Vec<u8> {
        vec![fill; KEY_LEN]
    }

    fn flipped(file: &[u8], index: usize) -> Vec<u8> {
        let mut copy = file.to_vec();
        copy[index] ^= 0x01;
        copy
    }

    #[test]
    fn seal_then_open_returns_the_plaintext() {
        let file = seal(&key(7), PLAINTEXT).unwrap();

        let opened = open(&key(7), &file).unwrap();

        assert_eq!(opened.as_slice(), PLAINTEXT);
    }

    #[test]
    fn sealed_file_has_the_versioned_layout_and_no_plaintext() {
        let file = seal(&key(7), PLAINTEXT).unwrap();

        assert_eq!(&file[..8], b"PAPANVLT");
        assert_eq!(file[8], 0x01);
        assert_eq!(file.len(), PLAINTEXT.len() + OVERHEAD);
        assert!(!file.windows(12).any(|window| window == b"s3cr3t-value"));
    }

    #[test]
    fn empty_plaintext_round_trips() {
        let file = seal(&key(7), b"").unwrap();

        assert_eq!(file.len(), OVERHEAD);
        assert!(open(&key(7), &file).unwrap().is_empty());
    }

    #[test]
    fn sealing_the_same_content_twice_gives_different_nonce_and_ciphertext() {
        let first = seal(&key(7), PLAINTEXT).unwrap();
        let second = seal(&key(7), PLAINTEXT).unwrap();

        assert_ne!(first[9..21], second[9..21]);
        assert_ne!(first[21..], second[21..]);
    }

    #[test]
    fn a_flipped_bit_anywhere_is_rejected() {
        let file = seal(&key(7), PLAINTEXT).unwrap();

        for index in 0..file.len() {
            let result = open(&key(7), &flipped(&file, index));
            let expected = if index < HEADER_LEN {
                CryptoError::BadFormat
            } else {
                CryptoError::Tampered
            };
            assert_eq!(result.unwrap_err(), expected, "byte {index}");
        }
    }

    #[test]
    fn header_is_authenticated_not_only_compared() {
        // A file sealed under a different header version must not open even if a future
        // reader accepted that version byte: the tag covers the header.
        let file = seal(&key(7), PLAINTEXT).unwrap();
        let cipher = cipher(&key(7)).unwrap();
        let mut other_header = header();
        other_header[8] = 0x02;
        let nonce = Nonce::try_assume_unique_for_key(&file[9..21]).unwrap();
        let mut body = file[21..].to_vec();

        let result = cipher.open_in_place(nonce, Aad::from(other_header), &mut body);

        assert!(result.is_err());
    }

    #[test]
    fn wrong_key_is_rejected() {
        let file = seal(&key(7), PLAINTEXT).unwrap();

        assert_eq!(open(&key(8), &file).unwrap_err(), CryptoError::Tampered);
    }

    #[test]
    fn truncated_or_foreign_bytes_are_bad_format() {
        let file = seal(&key(7), PLAINTEXT).unwrap();

        assert_eq!(open(&key(7), b"").unwrap_err(), CryptoError::BadFormat);
        assert_eq!(
            open(&key(7), &file[..OVERHEAD - 1]).unwrap_err(),
            CryptoError::BadFormat
        );
        assert_eq!(
            open(&key(7), &[b'x'; 64]).unwrap_err(),
            CryptoError::BadFormat
        );
        assert_eq!(
            open(&key(7), &file[..file.len() - 1]).unwrap_err(),
            CryptoError::Tampered
        );
    }

    #[test]
    fn keys_of_the_wrong_length_are_refused() {
        assert_eq!(
            seal(&[1u8; 16], PLAINTEXT).unwrap_err(),
            CryptoError::BadKey
        );
        assert_eq!(seal(&[], PLAINTEXT).unwrap_err(), CryptoError::BadKey);
        assert_eq!(
            open(&[1u8; 33], &[0u8; 64]).unwrap_err(),
            CryptoError::BadKey
        );
    }

    #[test]
    fn generated_keys_are_32_bytes_and_differ() {
        let first = generate_key().unwrap();
        let second = generate_key().unwrap();

        assert_eq!(first.len(), KEY_LEN);
        assert_ne!(first, second);
    }
}
