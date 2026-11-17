use super::super::fixtures::{collection, COLLECTION, OTHER_COLLECTION};
use super::*;
use crate::error::ErrorCode;
use crate::vars::vault::MemoryVault;

fn key(n: usize) -> String {
    format!("{n:064x}")
}

fn record(access: &str, saved_at_ms: u64) -> TokenRecord {
    TokenRecord {
        access_token: access.to_string(),
        refresh_token: Some(format!("refresh-of-{access}")),
        expires_at_ms: Some(saved_at_ms + 3_600_000),
        scope: "read".to_string(),
        token_url: "https://login.example.test/token".to_string(),
        saved_at_ms,
        verify_tls: true,
    }
}

fn owner(id: &str) -> Owner {
    Owner::of(&collection(id)).unwrap()
}

fn scratch() -> Owner {
    Owner::of(&RequestLocation::Scratch {
        id: "anything".to_string(),
    })
    .unwrap()
}

#[test]
fn owner_is_the_collection_id_or_scratch() {
    assert_eq!(owner(COLLECTION).as_str(), COLLECTION);
    assert_eq!(scratch().as_str(), "scratch");
}

#[test]
fn owner_refuses_a_collection_id_that_is_not_a_canonical_uuid() {
    let bad = [
        "scratch",
        "../x",
        "",
        "a/b",
        "7F1C2A34-5B6D-4E8F-9A0B-1C2D3E4F5A6B",
        "7f1c2a345b6d4e8f9a0b1c2d3e4f5a6b",
    ];

    for id in bad {
        let error = Owner::of(&collection(id)).unwrap_err();

        assert_eq!(error.code, ErrorCode::InvalidInput, "{id}");
    }
}

#[test]
fn a_saved_record_is_read_back_under_its_documented_ids() {
    let vault = MemoryVault::default();
    let saved = record("access-1", 1000);

    save(&vault, &owner(COLLECTION), &key(1), &saved).unwrap();

    assert_eq!(
        load(&vault, &owner(COLLECTION), &key(1)).unwrap(),
        Some(saved)
    );
    let stored = vault
        .get(&format!("o/{COLLECTION}/{}", key(1)))
        .unwrap()
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&stored).unwrap();
    assert_eq!(json["access_token"], "access-1");
    assert_eq!(json["token_url"], "https://login.example.test/token");
    assert_eq!(json["saved_at_ms"], 1000);
    let index = vault
        .get(&format!("o/{COLLECTION}/index"))
        .unwrap()
        .unwrap();
    let index: serde_json::Value = serde_json::from_str(&index).unwrap();
    assert_eq!(
        index,
        serde_json::json!([{ "key": key(1), "saved_at_ms": 1000 }])
    );
}

#[test]
fn optional_fields_are_omitted_from_the_stored_value() {
    let vault = MemoryVault::default();
    let mut bare = record("access-1", 5);
    bare.refresh_token = None;
    bare.expires_at_ms = None;

    save(&vault, &scratch(), &key(1), &bare).unwrap();

    let stored = vault
        .get(&format!("o/scratch/{}", key(1)))
        .unwrap()
        .unwrap();
    assert!(!stored.contains("refresh_token") && !stored.contains("expires_at_ms"));
    assert_eq!(load(&vault, &scratch(), &key(1)).unwrap(), Some(bare));
}

#[test]
fn another_owner_cannot_read_a_record() {
    let vault = MemoryVault::default();
    save(&vault, &owner(COLLECTION), &key(1), &record("access-1", 1)).unwrap();

    let other = load(&vault, &owner(OTHER_COLLECTION), &key(1)).unwrap();
    let from_scratch = load(&vault, &scratch(), &key(1)).unwrap();

    assert_eq!(other, None);
    assert_eq!(from_scratch, None);
}

#[test]
fn a_key_that_is_not_64_lowercase_hex_characters_is_refused() {
    let vault = MemoryVault::default();
    let bad = [
        "index",
        "",
        "../x",
        &"A".repeat(64),
        &"a".repeat(63),
        &"g".repeat(64),
    ];

    for key in bad {
        assert!(load(&vault, &scratch(), key).is_err(), "{key}");
        assert!(
            save(&vault, &scratch(), key, &record("a", 1)).is_err(),
            "{key}"
        );
        assert!(clear(&vault, &scratch(), key).is_err(), "{key}");
    }
}

#[test]
fn the_21st_record_evicts_the_oldest() {
    let vault = MemoryVault::default();
    let owner = owner(COLLECTION);
    for n in 1..=MAX_RECORDS {
        save(
            &vault,
            &owner,
            &key(n),
            &record(&format!("access-{n}"), 1000 + n as u64),
        )
        .unwrap();
    }

    save(&vault, &owner, &key(21), &record("access-21", 2000)).unwrap();

    assert_eq!(load(&vault, &owner, &key(1)).unwrap(), None);
    assert!(!vault.has(&format!("o/{COLLECTION}/{}", key(1))).unwrap());
    assert!(load(&vault, &owner, &key(2)).unwrap().is_some());
    assert!(load(&vault, &owner, &key(21)).unwrap().is_some());
    assert_eq!(read_index(&vault, &owner).unwrap().len(), MAX_RECORDS);
}

#[test]
fn saving_a_key_again_replaces_it_without_growing_the_index() {
    let vault = MemoryVault::default();
    let owner = owner(COLLECTION);
    for n in 1..=MAX_RECORDS {
        save(
            &vault,
            &owner,
            &key(n),
            &record(&format!("access-{n}"), 1000 + n as u64),
        )
        .unwrap();
    }

    save(&vault, &owner, &key(1), &record("renewed", 5000)).unwrap();
    save(&vault, &owner, &key(22), &record("access-22", 6000)).unwrap();

    let renewed = load(&vault, &owner, &key(1)).unwrap().unwrap();
    assert_eq!(renewed.access_token, "renewed");
    assert_eq!(load(&vault, &owner, &key(2)).unwrap(), None);
    assert_eq!(read_index(&vault, &owner).unwrap().len(), MAX_RECORDS);
}

#[test]
fn eviction_is_per_owner() {
    let vault = MemoryVault::default();
    save(&vault, &scratch(), &key(1), &record("scratch-token", 1)).unwrap();

    for n in 1..=MAX_RECORDS + 1 {
        save(
            &vault,
            &owner(COLLECTION),
            &key(n),
            &record("t", 100 + n as u64),
        )
        .unwrap();
    }

    assert!(load(&vault, &scratch(), &key(1)).unwrap().is_some());
}

#[test]
fn clear_removes_the_record_and_the_index_with_its_last_record() {
    let vault = MemoryVault::default();
    let owner = owner(COLLECTION);
    save(&vault, &owner, &key(1), &record("access-1", 1)).unwrap();
    save(&vault, &owner, &key(2), &record("access-2", 2)).unwrap();

    clear(&vault, &owner, &key(1)).unwrap();

    assert_eq!(load(&vault, &owner, &key(1)).unwrap(), None);
    assert_eq!(read_index(&vault, &owner).unwrap().len(), 1);

    clear(&vault, &owner, &key(2)).unwrap();
    clear(&vault, &owner, &key(2)).unwrap();

    assert!(!vault.has(&format!("o/{COLLECTION}/index")).unwrap());
    assert!(!vault.has(&format!("o/{COLLECTION}/{}", key(2))).unwrap());
}

#[test]
fn a_record_too_large_for_the_vault_is_refused_and_the_old_one_kept() {
    let vault = MemoryVault::default();
    let owner = owner(COLLECTION);
    save(&vault, &owner, &key(1), &record("old-token", 1)).unwrap();
    let huge = record(&"x".repeat(MAX_VALUE_BYTES), 2);

    let error = save(&vault, &owner, &key(1), &huge).unwrap_err();

    assert_eq!(error.message, "The token is too large to store.");
    assert!(!error.message.contains("xxx"));
    let kept = load(&vault, &owner, &key(1)).unwrap().unwrap();
    assert_eq!(kept.access_token, "old-token");
}

#[test]
fn a_damaged_record_or_index_reads_as_absent() {
    let vault = MemoryVault::default();
    let owner = owner(COLLECTION);
    vault
        .apply(&[
            SecretChange::Set {
                id: format!("o/{COLLECTION}/{}", key(1)),
                value: "not json".to_string(),
            },
            SecretChange::Set {
                id: format!("o/{COLLECTION}/index"),
                value: "[{\"key\":\"../x\",\"saved_at_ms\":1}, 7]".to_string(),
            },
        ])
        .unwrap();

    assert_eq!(load(&vault, &owner, &key(1)).unwrap(), None);
    assert!(read_index(&vault, &owner).unwrap().is_empty());

    save(&vault, &owner, &key(2), &record("access-2", 2)).unwrap();
    assert_eq!(read_index(&vault, &owner).unwrap().len(), 1);
}

#[test]
fn debug_output_never_shows_a_token() {
    let text = format!("{:?}", record("visible-access", 1));

    assert!(!text.contains("visible-access"));
    assert!(!text.contains("refresh-of"));
}
