//! Plan 04 §4 and §9: the encrypted vault file and its three modes. Everything runs on
//! `MemoryKeys`; the one test that touches the OS keychain is `#[ignore]`.

use std::fs;
use std::path::PathBuf;

use papan_lib::error::ErrorCode;
use papan_lib::vars::types::SecretStorageMode;
use papan_lib::vars::vault::{
    FileVault, KeyError, KeySource, MemoryKeys, OsKeys, SecretChange, SecretStore, MAX_VALUE_BYTES,
};

const SECRET: &str = "tok-SECRET-1234";
const HEADER_LEN: usize = 9;
const NONCE_LEN: usize = 12;

struct Fixture {
    _dir: tempfile::TempDir,
    path: PathBuf,
    keys: MemoryKeys,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.vault");
        Self {
            _dir: dir,
            path,
            keys: MemoryKeys::new(),
        }
    }

    /// A new vault over the same file and key store, as after an app restart.
    fn vault(&self) -> FileVault<MemoryKeys> {
        FileVault::new(self.path.clone(), self.keys.clone())
    }

    /// A fixture whose file already holds `g/token = SECRET`.
    fn with_secret() -> Self {
        let fixture = Self::new();
        fixture.vault().apply(&[set("g/token", SECRET)]).unwrap();
        fixture
    }

    fn bytes(&self) -> Vec<u8> {
        fs::read(&self.path).unwrap()
    }
}

fn set(id: &str, value: &str) -> SecretChange {
    SecretChange::Set {
        id: id.to_string(),
        value: value.to_string(),
    }
}

fn delete(id: &str) -> SecretChange {
    SecretChange::Delete { id: id.to_string() }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn mode(vault: &dyn SecretStore) -> SecretStorageMode {
    vault.status().mode
}

/// Opens `vault`, checks it is damaged, writes to it, and checks the file is untouched.
fn assert_damaged_and_untouched(fixture: &Fixture, before: &[u8]) {
    let vault = fixture.vault();

    assert_eq!(vault.get("g/token").unwrap(), None);
    assert_eq!(mode(&vault), SecretStorageMode::Damaged);
    assert!(vault.status().message.is_some());

    vault.apply(&[set("g/other", "kept-for-session")]).unwrap();
    vault.delete_prefix("g/").unwrap();
    vault.apply(&[set("g/again", "kept-for-session")]).unwrap();

    assert_eq!(
        vault.get("g/again").unwrap().as_deref(),
        Some("kept-for-session")
    );
    assert_eq!(fixture.bytes(), before);
    assert_eq!(mode(&vault), SecretStorageMode::Damaged);
}

#[test]
fn first_use_creates_a_key_and_writes_nothing_until_a_secret_is_set() {
    let fixture = Fixture::new();
    let vault = fixture.vault();

    assert!(!vault.has("g/token").unwrap());

    assert_eq!(mode(&vault), SecretStorageMode::Keychain);
    assert_eq!(vault.status().message, None);
    assert_eq!(fixture.keys.current().map(|key| key.len()), Some(32));
    assert!(!fixture.path.exists());

    vault.apply(&[set("g/token", SECRET)]).unwrap();

    assert!(fixture.path.exists());
    assert_eq!(vault.get("g/token").unwrap().as_deref(), Some(SECRET));
}

#[test]
fn constructing_and_asking_the_status_do_not_touch_the_key_store() {
    let fixture = Fixture::new();
    fixture
        .keys
        .set_failure(Some(KeyError::Denied("prompt refused".to_string())));
    let vault = fixture.vault();

    assert_eq!(mode(&vault), SecretStorageMode::Keychain);

    vault.has("g/token").unwrap();
    assert_eq!(mode(&vault), SecretStorageMode::SessionOnly);
}

#[test]
fn secrets_survive_a_restart() {
    let fixture = Fixture::with_secret();

    let reopened = fixture.vault();

    assert_eq!(reopened.get("g/token").unwrap().as_deref(), Some(SECRET));
    assert!(reopened.has("g/token").unwrap());
    assert_eq!(mode(&reopened), SecretStorageMode::Keychain);
}

#[test]
fn the_file_holds_neither_the_value_nor_the_id_in_clear() {
    let fixture = Fixture::with_secret();

    let bytes = fixture.bytes();

    assert_eq!(&bytes[..HEADER_LEN], b"PAPANVLT\x01");
    assert!(!contains(&bytes, SECRET.as_bytes()));
    assert!(!contains(&bytes, b"g/token"));
    assert!(!contains(&bytes, b"entries"));
}

#[test]
fn a_flipped_bit_in_any_part_of_the_file_is_damaged_and_leaves_the_file_alone() {
    let fixture = Fixture::with_secret();
    let original = fixture.bytes();
    let last = original.len() - 1;
    let places = [
        ("magic", 0),
        ("version", HEADER_LEN - 1),
        ("nonce", HEADER_LEN),
        ("ciphertext", HEADER_LEN + NONCE_LEN),
        ("tag", last),
    ];

    for (part, index) in places {
        let mut damaged = original.clone();
        damaged[index] ^= 0x01;
        fs::write(&fixture.path, &damaged).unwrap();

        assert_damaged_and_untouched(&fixture, &damaged);
        assert!(fixture.keys.current().is_some(), "{part}: key kept");
    }
}

#[test]
fn truncated_empty_and_foreign_files_are_damaged() {
    let fixture = Fixture::with_secret();
    let original = fixture.bytes();
    let variants: [&[u8]; 4] = [
        b"",
        &original[..20],
        &original[..original.len() - 1],
        b"hello",
    ];

    for variant in variants {
        fs::write(&fixture.path, variant).unwrap();

        assert_damaged_and_untouched(&fixture, variant);
    }
}

#[test]
fn wrong_key_is_damaged_and_leaves_file_and_key_alone() {
    let fixture = Fixture::with_secret();
    let original = fixture.bytes();
    fixture.keys.store(&[0x42; 32]).unwrap();

    assert_damaged_and_untouched(&fixture, &original);
    assert_eq!(fixture.keys.current(), Some(vec![0x42; 32]));
}

#[test]
fn a_key_of_the_wrong_length_is_damaged() {
    let fixture = Fixture::with_secret();
    let original = fixture.bytes();
    fixture.keys.store(&[]).unwrap();

    assert_damaged_and_untouched(&fixture, &original);

    let no_file = Fixture::new();
    no_file.keys.store(&[1; 16]).unwrap();
    let vault = no_file.vault();
    vault.apply(&[set("g/a", "1")]).unwrap();
    assert_eq!(mode(&vault), SecretStorageMode::Damaged);
    assert!(!no_file.path.exists());
    assert_eq!(no_file.keys.current(), Some(vec![1; 16]));
}

#[test]
fn a_file_without_a_key_is_damaged_and_no_key_is_created() {
    let fixture = Fixture::with_secret();
    let original = fixture.bytes();
    fixture.keys.delete().unwrap();

    assert_damaged_and_untouched(&fixture, &original);
    assert_eq!(fixture.keys.current(), None);
}

#[test]
fn a_key_without_a_file_is_an_empty_vault() {
    let fixture = Fixture::with_secret();
    let key = fixture.keys.current();
    fs::remove_file(&fixture.path).unwrap();

    let vault = fixture.vault();

    assert!(!vault.has("g/token").unwrap());
    assert_eq!(mode(&vault), SecretStorageMode::Keychain);
    assert_eq!(fixture.keys.current(), key);
    vault.apply(&[set("g/token", "new")]).unwrap();
    assert_eq!(
        fixture.vault().get("g/token").unwrap().as_deref(),
        Some("new")
    );
}

#[test]
fn a_file_damaged_while_the_app_runs_is_not_overwritten() {
    let fixture = Fixture::with_secret();
    let vault = fixture.vault();
    assert!(vault.has("g/token").unwrap());
    let mut damaged = fixture.bytes();
    damaged[HEADER_LEN + NONCE_LEN] ^= 0x80;
    fs::write(&fixture.path, &damaged).unwrap();

    vault.apply(&[set("g/new", "value")]).unwrap();

    assert_eq!(fixture.bytes(), damaged);
    assert_eq!(mode(&vault), SecretStorageMode::Damaged);
    assert_eq!(vault.get("g/new").unwrap().as_deref(), Some("value"));
    assert_eq!(vault.get("g/token").unwrap().as_deref(), Some(SECRET));
}

#[cfg(unix)]
#[test]
fn a_symlink_at_the_vault_path_is_damaged_and_its_target_is_untouched() {
    let other = Fixture::with_secret();
    let target_bytes = other.bytes();
    let fixture = Fixture::new();
    fixture.keys.store(&other.keys.current().unwrap()).unwrap();
    std::os::unix::fs::symlink(&other.path, &fixture.path).unwrap();
    let vault = fixture.vault();

    vault.apply(&[set("g/x", "1")]).unwrap();

    assert_eq!(mode(&vault), SecretStorageMode::Damaged);
    assert_eq!(vault.get("g/token").unwrap(), None);
    assert_eq!(other.bytes(), target_bytes);
    assert!(fs::symlink_metadata(&fixture.path).unwrap().is_symlink());
}

#[test]
fn every_write_uses_a_fresh_nonce() {
    let fixture = Fixture::new();
    let vault = fixture.vault();
    vault.apply(&[set("g/a", "1")]).unwrap();
    let first = fixture.bytes();

    vault.apply(&[set("g/a", "2")]).unwrap();
    vault.apply(&[set("g/a", "1")]).unwrap();
    let again = fixture.bytes();

    assert_eq!(first.len(), again.len());
    assert_ne!(
        first[HEADER_LEN..HEADER_LEN + NONCE_LEN],
        again[HEADER_LEN..HEADER_LEN + NONCE_LEN]
    );
    assert_ne!(
        first[HEADER_LEN + NONCE_LEN..],
        again[HEADER_LEN + NONCE_LEN..]
    );
}

#[test]
fn a_change_that_changes_nothing_does_not_write() {
    let fixture = Fixture::new();
    let vault = fixture.vault();

    vault.apply(&[]).unwrap();
    vault.apply(&[delete("g/none")]).unwrap();
    vault.delete_prefix("e/").unwrap();
    assert!(!fixture.path.exists());

    vault.apply(&[set("g/a", "1")]).unwrap();
    let before = fixture.bytes();
    vault.apply(&[set("g/a", "1")]).unwrap();
    vault.delete_prefix("e/").unwrap();
    assert_eq!(fixture.bytes(), before);
}

#[test]
fn unavailable_and_denied_key_stores_are_session_only_and_write_nothing() {
    let errors = [
        KeyError::Unavailable("no secret service".to_string()),
        KeyError::Denied("prompt refused".to_string()),
    ];
    for error in errors {
        let fixture = Fixture::new();
        fixture.keys.set_failure(Some(error));
        let vault = fixture.vault();

        vault.apply(&[set("g/token", SECRET)]).unwrap();
        vault.delete_prefix("e/").unwrap();

        assert_eq!(mode(&vault), SecretStorageMode::SessionOnly);
        assert!(vault.status().message.is_some());
        assert_eq!(vault.get("g/token").unwrap().as_deref(), Some(SECRET));
        assert!(!fixture.path.exists());
        assert_eq!(
            fs::read_dir(fixture.path.parent().unwrap())
                .unwrap()
                .count(),
            0
        );
        assert_eq!(fixture.vault().get("g/token").unwrap(), None);
    }
}

#[test]
fn an_existing_file_is_untouched_when_the_key_store_is_denied() {
    let fixture = Fixture::with_secret();
    let original = fixture.bytes();
    fixture
        .keys
        .set_failure(Some(KeyError::Denied("prompt refused".to_string())));
    let vault = fixture.vault();

    vault.apply(&[set("g/token", "other")]).unwrap();

    assert_eq!(mode(&vault), SecretStorageMode::SessionOnly);
    assert_eq!(fixture.bytes(), original);
}

/// Loads fine (no key yet) but cannot save one.
struct ReadOnlyKeys;

impl KeySource for ReadOnlyKeys {
    fn load(&self) -> Result<Option<Vec<u8>>, KeyError> {
        Ok(None)
    }
    fn store(&self, _key: &[u8]) -> Result<(), KeyError> {
        Err(KeyError::Denied("read only".to_string()))
    }
    fn delete(&self) -> Result<(), KeyError> {
        Ok(())
    }
}

#[test]
fn a_key_that_cannot_be_stored_on_first_use_is_session_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.vault");
    let vault = FileVault::new(path.clone(), ReadOnlyKeys);

    vault.apply(&[set("g/token", SECRET)]).unwrap();

    assert_eq!(mode(&vault), SecretStorageMode::SessionOnly);
    assert_eq!(vault.get("g/token").unwrap().as_deref(), Some(SECRET));
    assert!(!path.exists());
}

#[test]
fn reset_removes_file_and_key_and_starts_an_empty_vault() {
    let fixture = Fixture::with_secret();
    let old_key = fixture.keys.current().unwrap();
    fs::write(&fixture.path, b"garbage").unwrap();
    let vault = fixture.vault();
    vault.apply(&[set("g/session", "x")]).unwrap();
    assert_eq!(mode(&vault), SecretStorageMode::Damaged);

    vault.reset().unwrap();

    assert!(!fixture.path.exists());
    assert_eq!(mode(&vault), SecretStorageMode::Keychain);
    assert!(!vault.has("g/session").unwrap());
    let new_key = fixture.keys.current().unwrap();
    assert_eq!(new_key.len(), 32);
    assert_ne!(new_key, old_key);

    vault.apply(&[set("g/token", "fresh")]).unwrap();
    assert_eq!(
        fixture.vault().get("g/token").unwrap().as_deref(),
        Some("fresh")
    );
}

#[test]
fn reset_of_a_healthy_vault_forgets_every_secret() {
    let fixture = Fixture::with_secret();
    let vault = fixture.vault();

    vault.reset().unwrap();

    assert!(!vault.has("g/token").unwrap());
    assert!(!fixture.path.exists());
    assert!(!fixture.vault().has("g/token").unwrap());
}

#[test]
fn reset_without_a_key_store_removes_the_file_and_stays_session_only() {
    let fixture = Fixture::with_secret();
    fixture
        .keys
        .set_failure(Some(KeyError::Unavailable("gone".to_string())));
    let vault = fixture.vault();

    vault.reset().unwrap();

    assert!(!fixture.path.exists());
    assert_eq!(mode(&vault), SecretStorageMode::SessionOnly);
}

#[test]
fn apply_writes_all_changes_together_including_move_and_delete() {
    let fixture = Fixture::new();
    let vault = fixture.vault();
    vault
        .apply(&[set("c/id/a", "1"), set("c/id/b", "2"), set("c/id/c", "3")])
        .unwrap();

    vault
        .apply(&[
            SecretChange::Move {
                from: "c/id/a".to_string(),
                to: "c/id/renamed".to_string(),
            },
            SecretChange::Move {
                from: "c/id/missing".to_string(),
                to: "c/id/c".to_string(),
            },
            delete("c/id/b"),
        ])
        .unwrap();

    let reopened = fixture.vault();
    assert_eq!(reopened.get("c/id/renamed").unwrap().as_deref(), Some("1"));
    assert_eq!(reopened.get("c/id/c").unwrap().as_deref(), Some("3"));
    assert!(!reopened.has("c/id/a").unwrap());
    assert!(!reopened.has("c/id/b").unwrap());
}

#[cfg(unix)]
#[test]
fn a_failed_write_changes_nothing_in_memory_or_on_disk() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::with_secret();
    let vault = fixture.vault();
    let original = fixture.bytes();
    let parent = fixture.path.parent().unwrap();
    // No temp file can be created in a read-only directory, so the write fails.
    fs::set_permissions(parent, fs::Permissions::from_mode(0o500)).unwrap();

    let error = vault
        .apply(&[set("g/a", "new-SECRET-value"), delete("g/token")])
        .unwrap_err();

    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(error.code, ErrorCode::Io);
    assert!(!format!("{error:?}").contains("new-SECRET-value"));
    assert_eq!(mode(&vault), SecretStorageMode::Keychain);
    assert_eq!(vault.get("g/a").unwrap(), None);
    assert_eq!(vault.get("g/token").unwrap().as_deref(), Some(SECRET));
    assert_eq!(fixture.bytes(), original);

    // The same vault works again once the directory is writable.
    vault.apply(&[set("g/b", "2")]).unwrap();
    let reopened = fixture.vault();
    assert_eq!(reopened.get("g/token").unwrap().as_deref(), Some(SECRET));
    assert_eq!(reopened.get("g/a").unwrap(), None);
    assert_eq!(reopened.get("g/b").unwrap().as_deref(), Some("2"));
}

#[test]
fn something_that_is_not_a_file_at_the_vault_path_is_damaged_and_kept() {
    let fixture = Fixture::with_secret();
    let vault = fixture.vault();
    assert!(vault.has("g/token").unwrap());
    fs::remove_file(&fixture.path).unwrap();
    fs::create_dir(&fixture.path).unwrap();
    fs::write(fixture.path.join("keep"), b"x").unwrap();

    vault.apply(&[set("g/a", "1")]).unwrap();

    assert_eq!(mode(&vault), SecretStorageMode::Damaged);
    assert_eq!(fs::read(fixture.path.join("keep")).unwrap(), b"x");
    assert_eq!(mode(&fixture.vault()), SecretStorageMode::Keychain);
    assert!(fixture.vault().get("g/a").is_ok());
}

#[test]
fn an_invalid_change_refuses_the_whole_batch() {
    let fixture = Fixture::new();
    let vault = fixture.vault();
    let too_long = "x".repeat(MAX_VALUE_BYTES + 1);

    let error = vault
        .apply(&[set("g/ok", "1"), set("g/big", &too_long)])
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(!vault.has("g/ok").unwrap());
    assert!(!fixture.path.exists());
    assert_eq!(
        vault.apply(&[set("", "1")]).unwrap_err().code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn a_value_of_exactly_64_kib_is_stored() {
    let fixture = Fixture::new();
    let value = "é".repeat(MAX_VALUE_BYTES / 2);

    fixture.vault().apply(&[set("g/big", &value)]).unwrap();

    assert_eq!(fixture.vault().get("g/big").unwrap(), Some(value));
}

#[test]
fn a_vault_over_5_mib_is_refused_and_keeps_what_it_had() {
    let fixture = Fixture::with_secret();
    let vault = fixture.vault();
    let before = fixture.bytes();
    let value = "x".repeat(MAX_VALUE_BYTES);
    let changes: Vec<SecretChange> = (0..81)
        .map(|index| set(&format!("g/big{index}"), &value))
        .collect();

    let error = vault.apply(&changes).unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert_eq!(fixture.bytes(), before);
    assert!(!vault.has("g/big0").unwrap());
    assert_eq!(vault.get("g/token").unwrap().as_deref(), Some(SECRET));
}

#[test]
fn delete_prefix_removes_one_environment_only() {
    let fixture = Fixture::new();
    let vault = fixture.vault();
    vault
        .apply(&[
            set("e/id/dev/a", "1"),
            set("e/id/dev/b/c", "2"),
            set("e/id/dev2/a", "3"),
            set("e/id2/dev/a", "4"),
            set("c/id/dev/a", "5"),
        ])
        .unwrap();

    vault.delete_prefix("e/id/dev/").unwrap();

    let reopened = fixture.vault();
    assert!(!reopened.has("e/id/dev/a").unwrap());
    assert!(!reopened.has("e/id/dev/b/c").unwrap());
    assert_eq!(reopened.get("e/id/dev2/a").unwrap().as_deref(), Some("3"));
    assert_eq!(reopened.get("e/id2/dev/a").unwrap().as_deref(), Some("4"));
    assert_eq!(reopened.get("c/id/dev/a").unwrap().as_deref(), Some("5"));
}

#[test]
fn a_write_keeps_what_another_instance_wrote() {
    let fixture = Fixture::new();
    let first = fixture.vault();
    let second = fixture.vault();
    first.apply(&[set("g/a", "1")]).unwrap();
    second.apply(&[set("g/b", "2")]).unwrap();

    first.apply(&[set("g/c", "3")]).unwrap();

    assert_eq!(first.get("g/b").unwrap().as_deref(), Some("2"));
    let reopened = fixture.vault();
    for (id, value) in [("g/a", "1"), ("g/b", "2"), ("g/c", "3")] {
        assert_eq!(reopened.get(id).unwrap().as_deref(), Some(value));
    }
}

#[test]
fn the_vault_is_shareable_across_threads() {
    let fixture = Fixture::new();
    let vault: std::sync::Arc<dyn SecretStore> = std::sync::Arc::new(fixture.vault());

    let handles: Vec<_> = (0..8)
        .map(|index| {
            let vault = std::sync::Arc::clone(&vault);
            std::thread::spawn(move || vault.apply(&[set(&format!("g/{index}"), "v")]).unwrap())
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }

    let reopened = fixture.vault();
    assert!((0..8).all(|index| reopened.has(&format!("g/{index}")).unwrap()));
}

#[cfg(unix)]
#[test]
fn the_file_is_readable_by_its_owner_only_and_no_temp_file_is_left() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::with_secret();
    fixture.vault().apply(&[set("g/more", "2")]).unwrap();

    let mode = fs::metadata(&fixture.path).unwrap().permissions().mode();

    assert_eq!(mode & 0o777, 0o600);
    assert_eq!(
        fs::read_dir(fixture.path.parent().unwrap())
            .unwrap()
            .count(),
        1
    );
}

/// Touches the real OS keychain under a test-only account. Run by hand on each OS:
/// `cargo test --test vault -- --ignored`.
#[test]
#[ignore = "uses the real OS keychain"]
fn os_keychain_round_trip() {
    let account = "secret-vault-key-test";
    let keys = OsKeys::for_account(account);
    keys.delete().unwrap();
    assert_eq!(keys.load().unwrap(), None);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.vault");
    let vault = FileVault::new(path.clone(), keys);
    vault.apply(&[set("g/token", SECRET)]).unwrap();
    assert_eq!(mode(&vault), SecretStorageMode::Keychain);

    let reopened = FileVault::new(path.clone(), OsKeys::for_account(account));
    assert_eq!(reopened.get("g/token").unwrap().as_deref(), Some(SECRET));
    assert_eq!(
        OsKeys::for_account(account)
            .load()
            .unwrap()
            .map(|key| key.len()),
        Some(32)
    );

    reopened.reset().unwrap();
    assert!(!path.exists());
    // `reset` opens again and creates a fresh key; remove it so nothing is left behind.
    OsKeys::for_account(account).delete().unwrap();
    assert_eq!(OsKeys::for_account(account).load().unwrap(), None);
}
