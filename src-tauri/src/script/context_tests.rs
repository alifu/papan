use super::*;

fn layer(pairs: &[(&str, &str)], secrets: &[&str]) -> VarLayer {
    VarLayer {
        values: pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        secrets: secrets.iter().map(|s| s.to_string()).collect(),
    }
}

fn context() -> ScriptContext {
    ScriptContext::new(
        Layers {
            globals: layer(&[("a", "global"), ("g", "G")], &[]),
            collection: layer(&[("a", "collection"), ("c", "C")], &[]),
            environment: Some(layer(&[("a", "env"), ("token", "s3cret")], &["token"])),
            runtime: VarLayer::default(),
        },
        ScriptRequest::default(),
    )
}

#[test]
fn lookup_reads_the_highest_scope_first() {
    let mut ctx = context();
    assert_eq!(ctx.lookup("a"), Some("env"));
    assert_eq!(ctx.lookup("c"), Some("C"));
    assert_eq!(ctx.lookup("g"), Some("G"));
    assert_eq!(ctx.lookup("missing"), None);

    ctx.set(Scope::Runtime, "a", "runtime").unwrap();
    assert_eq!(ctx.lookup("a"), Some("runtime"));
    assert_eq!(ctx.merged()["a"], "runtime");
    assert_eq!(ctx.merged()["g"], "G");
}

#[test]
fn changes_list_sets_and_unsets_against_the_baseline() {
    let mut ctx = context();
    ctx.set(Scope::Environment, "token", "new").unwrap();
    ctx.set(Scope::Globals, "g", "G").unwrap(); // same value: not a change
    ctx.set(Scope::Collection, "fresh", "1").unwrap();
    ctx.unset(Scope::Globals, "a").unwrap();
    ctx.set(Scope::Runtime, "r", "1").unwrap();
    ctx.set(Scope::Runtime, "r", "2").unwrap();

    let changes = ctx.changes();
    let brief: Vec<_> = changes
        .iter()
        .map(|c| (c.scope, c.name.as_str(), c.value.as_deref(), c.secret))
        .collect();
    assert_eq!(
        brief,
        vec![
            (Scope::Globals, "a", None, false),
            (Scope::Collection, "fresh", Some("1"), false),
            (Scope::Environment, "token", Some("new"), true),
            (Scope::Runtime, "r", Some("2"), false),
        ]
    );
}

#[test]
fn environment_writes_fail_without_an_environment() {
    let mut ctx = ScriptContext::default();
    assert!(ctx.set(Scope::Environment, "a", "b").is_err());
    assert!(ctx.layer(Scope::Environment).is_none());
}

#[test]
fn set_rejects_bad_names_and_oversized_values() {
    let mut ctx = context();
    let before = ctx.clone();
    assert!(ctx.set(Scope::Globals, "", "v").is_err());
    assert!(ctx
        .set(Scope::Globals, &"n".repeat(MAX_NAME_CHARS + 1), "v")
        .is_err());
    assert!(ctx.set(Scope::Globals, "a\nb", "v").is_err());
    assert!(ctx
        .set(Scope::Globals, "big", &"v".repeat(MAX_VALUE_BYTES + 1))
        .is_err());
    assert_eq!(ctx, before);
    assert!(ctx
        .set(Scope::Globals, &"n".repeat(MAX_NAME_CHARS), "v")
        .is_ok());
    assert!(ctx
        .set(Scope::Globals, "big", &"v".repeat(MAX_VALUE_BYTES))
        .is_ok());
}

#[test]
fn a_scope_is_capped_in_count_and_bytes() {
    let mut ctx = ScriptContext::default();
    for i in 0..MAX_VARS_PER_SCOPE {
        ctx.set(Scope::Runtime, &format!("v{i}"), "").unwrap();
    }
    assert!(ctx.set(Scope::Runtime, "one-more", "").is_err());
    assert!(ctx.set(Scope::Runtime, "v0", "updated").is_ok());

    let mut ctx = ScriptContext::default();
    let value = "x".repeat(MAX_VALUE_BYTES);
    let fits = MAX_SCOPE_BYTES / (MAX_VALUE_BYTES + 8);
    let results: Vec<bool> = (0..fits + 2)
        .map(|i| ctx.set(Scope::Globals, &format!("k{i:06}"), &value).is_ok())
        .collect();
    assert!(results[0]);
    assert!(!results[fits + 1]);
    assert!(ctx.set(Scope::Globals, "k000000", "small").is_ok());
}

#[test]
fn secret_values_cover_old_new_and_extra_secrets() {
    let mut ctx = context();
    ctx.extra_secrets = vec!["auth-pass".into(), String::new()];
    ctx.set(Scope::Environment, "token", "rotated").unwrap();
    assert_eq!(ctx.secret_values(), vec!["auth-pass", "rotated", "s3cret"]);
}

#[test]
fn validate_request_rejects_injection_and_oversize() {
    let ok = ScriptRequest {
        method: "POST".into(),
        url: "https://example.test/a".into(),
        headers: vec![("X-A".into(), "1".into())],
        body: Some("{}".into()),
    };
    assert_eq!(validate_request(&ok), Ok(()));
    let with_headers = |headers| ScriptRequest {
        headers,
        ..ok.clone()
    };
    let bad = [
        ScriptRequest {
            method: "GET /x".into(),
            ..ok.clone()
        },
        ScriptRequest {
            method: String::new(),
            ..ok.clone()
        },
        ScriptRequest {
            url: "http://a/\r\nHost: b".into(),
            ..ok.clone()
        },
        ScriptRequest {
            url: "u".repeat(MAX_VALUE_BYTES + 1),
            ..ok.clone()
        },
        with_headers(vec![("X A".into(), "1".into())]),
        with_headers(vec![("X-A".into(), "1\r\nB: 2".into())]),
        with_headers(vec![("X".into(), "1".into()); MAX_HEADERS + 1]),
        ScriptRequest {
            body: Some("b".repeat(MAX_BODY_BYTES + 1)),
            ..ok.clone()
        },
    ];
    for (index, request) in bad.iter().enumerate() {
        assert!(validate_request(request).is_err(), "case {index}");
    }
}
