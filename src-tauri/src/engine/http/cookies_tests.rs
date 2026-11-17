use super::*;

const FUTURE_MS: f64 = 4_102_444_800_000.0; // 2100-01-01

fn input(domain: &str, path: &str, name: &str, value: &str) -> CookieInput {
    CookieInput {
        name: name.into(),
        value: value.into(),
        domain: domain.into(),
        path: path.into(),
        expires_at_ms: None,
        http_only: false,
        secure: false,
    }
}

fn filled(cookies: &[CookieInput]) -> CookieStore {
    let mut store = CookieStore::default();
    for cookie in cookies {
        set(&mut store, cookie).unwrap();
    }
    store
}

fn triples(store: &CookieStore) -> Vec<(String, String, String)> {
    list(store)
        .into_iter()
        .map(|c| (c.domain, c.path, c.name))
        .collect()
}

#[test]
fn cookies_are_listed_by_domain_path_and_name() {
    let store = filled(&[
        input("b.test", "/", "a", "1"),
        input("a.test", "/x", "b", "2"),
        input("a.test", "/", "z", "3"),
        input("a.test", "/", "c", "4"),
    ]);

    let names: Vec<String> = triples(&store).into_iter().map(|(_, _, n)| n).collect();

    assert_eq!(names, ["c", "z", "b", "a"]);
}

#[test]
fn set_replaces_the_cookie_with_the_same_domain_path_and_name() {
    let store = filled(&[
        input("a.test", "/", "sid", "old"),
        CookieInput {
            expires_at_ms: Some(FUTURE_MS),
            http_only: true,
            secure: true,
            ..input("A.Test", "/", "sid", "new")
        },
    ]);

    let listed = list(&store);

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].value, "new");
    assert_eq!(listed[0].domain, "a.test");
    assert_eq!(listed[0].expires_at_ms, Some(FUTURE_MS));
    assert!(listed[0].http_only && listed[0].secure);
}

#[test]
fn delete_removes_a_whole_domain_or_one_cookie() {
    let mut store = filled(&[
        input("a.test", "/", "x", "1"),
        input("a.test", "/p", "y", "2"),
        input("b.test", "/", "x", "3"),
    ]);

    let one = CookieFilter {
        domain: "b.test".into(),
        path: Some("/".into()),
        name: Some("x".into()),
    };
    let removed_one = delete(&mut store, &one).unwrap();
    let domain = CookieFilter {
        domain: "A.TEST".into(),
        path: None,
        name: None,
    };
    let removed_domain = delete(&mut store, &domain).unwrap();

    assert_eq!((removed_one, removed_domain), (1, 2));
    assert!(list(&store).is_empty());
}

#[test]
fn a_filter_with_only_a_path_or_only_a_name_is_refused() {
    let mut store = filled(&[input("a.test", "/", "x", "1")]);
    let filter = CookieFilter {
        domain: "a.test".into(),
        path: None,
        name: Some("x".into()),
    };

    let error = delete(&mut store, &filter).unwrap_err();

    assert_eq!(error.code, crate::error::ErrorCode::InvalidInput);
    assert_eq!(list(&store).len(), 1);
}

#[test]
fn expired_cookies_are_hidden() {
    let mut store = CookieStore::default();
    let url = Url::parse("https://a.test/").unwrap();
    store.parse("old=1", &url).unwrap();
    store.parse("new=2", &url).unwrap();
    // An expired copy expires the stored cookie, which stays in the store.
    store.parse("old=1; Max-Age=0", &url).unwrap();
    assert_eq!(store.iter_any().count(), 2);

    let names: Vec<String> = list(&store).into_iter().map(|c| c.name).collect();

    assert_eq!(names, ["new"]);
}

#[test]
fn invalid_cookies_are_refused_and_nothing_is_stored() {
    let past = CookieInput {
        expires_at_ms: Some(1_000.0),
        ..input("a.test", "/", "n", "v")
    };
    let not_finite = CookieInput {
        expires_at_ms: Some(f64::NAN),
        ..input("a.test", "/", "n", "v")
    };
    let cases = [
        input("a.test", "/", "", "v"),
        input("a.test", "/", "a b", "v"),
        input("a.test", "/", "a=b", "v"),
        input("a.test", "/", "a;b", "v"),
        input("a.test", "/", "n", "v;w"),
        input("a.test", "/", "n", "v,w"),
        input("a.test", "/", "n", "v\nw"),
        input("a.test", "p", "n", "v"),
        input("a.test", "/;x", "n", "v"),
        input("https://a.test", "/", "n", "v"),
        input("a.test:8080", "/", "n", "v"),
        input("", "/", "n", "v"),
        past,
        not_finite,
    ];
    let mut store = CookieStore::default();

    for case in &cases {
        let error = set(&mut store, case).unwrap_err();
        assert_eq!(
            error.code,
            crate::error::ErrorCode::InvalidInput,
            "{case:?}"
        );
    }

    assert!(list(&store).is_empty());
}
