use super::import;

#[test]
fn after_an_unknown_option_the_last_url_is_used_and_named() {
    let imported =
        import("curl --new-flag https://evil.example https://api.example.com/x").unwrap();

    assert_eq!(imported.content.url, "https://api.example.com/x");
    assert_eq!(
        imported.warnings,
        [
            "unsupported option: --new-flag",
            "several URLs after an unknown option; using the last one: https://api.example.com/x",
        ]
    );
}

#[test]
fn without_unknown_options_the_first_url_is_used() {
    let imported = import("curl https://a.example https://b.example").unwrap();

    assert_eq!(imported.content.url, "https://a.example");
    assert_eq!(imported.warnings, ["only the first URL is imported"]);
}

#[test]
fn insecure_is_imported_with_a_warning() {
    let imported = import("curl -k https://a.example").unwrap();

    assert!(!imported.content.settings.verify_tls);
    assert_eq!(
        imported.warnings,
        ["-k: TLS certificate verification is turned off for this request"]
    );
}

#[test]
fn data_urlencode_splits_at_the_first_equals_sign_like_curl() {
    let imported = import("curl --data-urlencode 'a@b=c d' https://a.example").unwrap();

    assert!(imported.warnings.is_empty());
    assert_eq!(
        imported.content.body,
        crate::store::types::Body::Text {
            text: "a@b=c%20d".into()
        }
    );
}
