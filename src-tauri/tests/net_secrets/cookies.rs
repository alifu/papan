//! Persisted cookie jars (plan 03b §6.5, §15.5).

use super::*;

#[test]
fn persistent_cookies_survive_a_restart_and_session_cookies_do_not() {
    let app = AppData::new();

    cookie_file::save(&app.cookies(), JAR, &app.vault(), &persistent()).unwrap();
    let loaded = cookie_file::load(&app.cookies(), JAR, &app.vault())
        .unwrap()
        .unwrap();

    assert!(has(&loaded, "sid") && !has(&loaded, "tmp"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(app.jar_file()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let key = app.vault().get(COOKIE_KEY_ID).unwrap().unwrap();
    assert_eq!(key.len(), 44);
    let sealed_as_vault = sealed::open(b"PAPANVLT", &[0u8; 32], &fs::read(app.jar_file()).unwrap());
    assert_eq!(sealed_as_vault.unwrap_err(), sealed::CryptoError::BadFormat);
}

#[test]
fn the_vault_is_not_opened_without_a_jar_file_or_a_persistent_cookie() {
    let app = AppData::new();
    let spy = Spy::default();

    let loaded = cookie_file::load(&app.cookies(), JAR, &spy).unwrap();
    cookie_file::save(&app.cookies(), JAR, &spy, &jar_with(&["tmp=1"])).unwrap();

    assert!(loaded.is_none());
    assert_eq!(spy.reads.load(Ordering::SeqCst), 0);
    assert!(!app.jar_file().exists());
}

#[test]
fn saving_a_jar_without_persistent_cookies_removes_its_file() {
    let app = AppData::new();
    cookie_file::save(&app.cookies(), JAR, &app.vault(), &persistent()).unwrap();

    cookie_file::save(&app.cookies(), JAR, &app.vault(), &jar_with(&["tmp=1"])).unwrap();

    assert!(!app.jar_file().exists());
}

#[test]
fn without_a_keychain_no_jar_file_and_no_cookie_key_are_written() {
    let keys = MemoryKeys::failing(KeyError::Unavailable("no keychain".to_string()));
    let app = AppData::with_keys(keys);
    let damaged = MemoryVault::new(SecretStorageMode::Damaged);

    cookie_file::save(&app.cookies(), JAR, &app.vault(), &persistent()).unwrap();
    cookie_file::save(&app.cookies(), JAR, &damaged, &persistent()).unwrap();

    assert!(!app.jar_file().exists());
    assert!(!damaged.has(COOKIE_KEY_ID).unwrap());
    assert!(cookie_file::load(&app.cookies(), JAR, &damaged)
        .unwrap()
        .is_none());
}

#[test]
fn a_jar_file_that_cannot_be_opened_is_ignored_and_replaced_by_the_next_save() {
    let app = AppData::new();
    cookie_file::save(&app.cookies(), JAR, &app.vault(), &persistent()).unwrap();
    let mut bytes = fs::read(app.jar_file()).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    let too_big = vec![0u8; cookie_file::MAX_JAR_BYTES + sealed::OVERHEAD + 1];

    for damaged in [bytes.as_slice(), b"not a jar", too_big.as_slice()] {
        fs::write(app.jar_file(), damaged).unwrap();
        assert!(cookie_file::load(&app.cookies(), JAR, &app.vault())
            .unwrap()
            .is_none());
    }
    cookie_file::save(&app.cookies(), JAR, &app.vault(), &persistent()).unwrap();

    assert!(cookie_file::load(&app.cookies(), JAR, &app.vault())
        .unwrap()
        .is_some());
}

#[test]
fn a_stored_cookie_key_of_the_wrong_size_is_replaced() {
    let app = AppData::new();
    cookie_file::save(&app.cookies(), JAR, &app.vault(), &persistent()).unwrap();
    app.vault()
        .apply(&[set(COOKIE_KEY_ID, "c2hvcnQ=")])
        .unwrap();

    let before = cookie_file::load(&app.cookies(), JAR, &app.vault()).unwrap();
    cookie_file::save(&app.cookies(), JAR, &app.vault(), &persistent()).unwrap();

    assert!(before.is_none());
    assert_eq!(app.vault().get(COOKIE_KEY_ID).unwrap().unwrap().len(), 44);
    assert!(cookie_file::load(&app.cookies(), JAR, &app.vault())
        .unwrap()
        .is_some());
}

#[cfg(unix)]
#[test]
fn a_symlink_at_the_jar_path_is_not_read() {
    let app = AppData::new();
    cookie_file::save(&app.cookies(), JAR, &app.vault(), &persistent()).unwrap();
    let real = app.cookies().join("elsewhere");
    fs::rename(app.jar_file(), &real).unwrap();
    std::os::unix::fs::symlink(&real, app.jar_file()).unwrap();

    assert!(cookie_file::load(&app.cookies(), JAR, &app.vault())
        .unwrap()
        .is_none());
}

#[test]
fn jar_names_that_are_not_built_in_rust_are_rejected() {
    let app = AppData::new();
    fs::create_dir_all(app.cookies()).unwrap();
    let outside = app.write("x.jar", b"keep");
    let unverified = format!("{JAR}#tls-unverified");

    for name in [
        "../x",
        "",
        "collection-",
        "collection-../x",
        "scratch/..",
        &unverified,
    ] {
        let saved = cookie_file::save(&app.cookies(), name, &app.vault(), &persistent());

        assert_eq!(saved.unwrap_err().code, ErrorCode::InvalidInput, "{name}");
        assert!(cookie_file::load(&app.cookies(), name, &app.vault()).is_err());
        assert!(cookie_file::delete(&app.cookies(), name).is_err(), "{name}");
    }
    assert_eq!(fs::read(outside).unwrap(), b"keep");
    assert_eq!(fs::read_dir(app.cookies()).unwrap().count(), 0);
}

#[test]
fn delete_removes_the_jar_file_and_a_missing_file_is_not_an_error() {
    let app = AppData::new();
    cookie_file::save(&app.cookies(), "scratch", &app.vault(), &persistent()).unwrap();

    cookie_file::delete(&app.cookies(), "scratch").unwrap();
    cookie_file::delete(&app.cookies(), "scratch").unwrap();

    assert!(cookie_file::load(&app.cookies(), "scratch", &app.vault())
        .unwrap()
        .is_none());
    assert_eq!(fs::read_dir(app.cookies()).unwrap().count(), 0);
}

#[test]
fn a_jar_file_that_cannot_be_read_for_lack_of_the_key_is_an_error() {
    let app = AppData::new();
    cookie_file::save(&app.cookies(), JAR, &app.vault(), &persistent()).unwrap();
    let session_only = MemoryVault::new(SecretStorageMode::SessionOnly);
    let damaged = MemoryVault::new(SecretStorageMode::Damaged);

    let denied = cookie_file::load(&app.cookies(), JAR, &session_only);
    let unreadable = cookie_file::load(&app.cookies(), JAR, &damaged);

    assert!(denied.is_err() && unreadable.is_err());
    assert!(cookie_file::load(&app.cookies(), JAR, &app.vault())
        .unwrap()
        .is_some());
}

#[test]
fn a_jar_file_does_not_open_under_another_jar_name() {
    let app = AppData::new();
    cookie_file::save(&app.cookies(), JAR, &app.vault(), &persistent()).unwrap();
    let other = app.cookies().join("scratch.jar");
    fs::copy(app.jar_file(), &other).unwrap();

    let swapped = cookie_file::load(&app.cookies(), "scratch", &app.vault()).unwrap();

    assert!(swapped.is_none());
}
