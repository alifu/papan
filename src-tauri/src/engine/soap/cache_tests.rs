use super::*;

fn entry(scope: &str, file_name: &str) -> CacheEntry {
    CacheEntry {
        scope: scope.into(),
        source: file_name.into(),
        file_name: file_name.into(),
        fetched: vec!["calc.wsdl".into()],
        warnings: Vec::new(),
        fetched_at_ms: 1.0,
        model: WsdlModel::default(),
    }
}

#[test]
fn keys_are_hex_sha256_of_scope_and_source() {
    let a = key("collection:c1", "http://x/a?wsdl");

    assert_eq!(a.len(), 64);
    assert!(validate_key(&a).is_ok());
    assert_ne!(a, key("collection:c2", "http://x/a?wsdl"));
    assert_eq!(a, key("collection:c1", "http://x/a?wsdl"));
}

#[test]
fn a_key_that_is_not_64_hex_digits_is_refused() {
    for bad in [
        "",
        "../x",
        &"A".repeat(64),
        &"0".repeat(63),
        &format!("{}/", "0".repeat(63)),
    ] {
        assert!(validate_key(bad).is_err(), "{bad}");
    }
}

#[test]
fn stores_and_loads_an_entry_and_finds_files_by_scope_and_name() {
    let dir = tempfile::tempdir().unwrap();
    let cache = WsdlCache::new(dir.path());
    let id = key("scratch:s", "file:/tmp/calc.wsdl");

    cache.store(&id, &entry("scratch:s", "calc.wsdl")).unwrap();

    assert_eq!(
        cache.load(&id).unwrap(),
        Some(entry("scratch:s", "calc.wsdl"))
    );
    assert_eq!(
        cache.find_file("scratch:s", "calc.wsdl").map(|(k, _)| k),
        Some(id)
    );
    assert!(cache.find_file("scratch:other", "calc.wsdl").is_none());
    assert!(cache.find_file("scratch:s", "").is_none());
    assert!(cache.load(&key("scratch:s", "missing")).unwrap().is_none());
    assert!(dir.path().join("wsdl").is_dir());
}

#[test]
fn a_damaged_entry_is_deleted_and_reads_as_no_cache() {
    let dir = tempfile::tempdir().unwrap();
    let cache = WsdlCache::new(dir.path());
    let id = key("s", "x");
    std::fs::create_dir_all(dir.path().join("wsdl")).unwrap();
    std::fs::write(
        dir.path().join("wsdl").join(format!("{id}.json")),
        "{not json",
    )
    .unwrap();

    assert!(cache.load(&id).unwrap().is_none());
    assert!(!dir.path().join("wsdl").join(format!("{id}.json")).exists());
}

#[test]
fn keeps_at_most_one_hundred_entries_dropping_the_oldest() {
    let dir = tempfile::tempdir().unwrap();
    let cache = WsdlCache::new(dir.path());
    for index in 0..=MAX_ENTRIES {
        cache
            .store(&key("s", &index.to_string()), &entry("s", "x"))
            .unwrap();
    }

    let count = std::fs::read_dir(dir.path().join("wsdl")).unwrap().count();
    assert_eq!(count, MAX_ENTRIES);
    assert!(cache
        .load(&key("s", &MAX_ENTRIES.to_string()))
        .unwrap()
        .is_some());
}
