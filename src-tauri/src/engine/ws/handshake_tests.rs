use super::*;

fn url(text: &str) -> Url {
    Url::parse(text).unwrap()
}

#[test]
fn debug_output_hides_credentials() {
    let mut spec = WsConnectSpec::new(url("wss://u:pw@x.test/a?token=abc"));
    spec.headers = vec![(
        HeaderName::from_static("authorization"),
        HeaderValue::from_static("Bearer SECRETTOKEN"),
    )];
    spec.proxy = Some(ProxyHop {
        host: "p".into(),
        port: 1,
        authorization: Some("Basic PROXYSECRET".into()),
    });
    spec.secrets = vec!["SECRETTOKEN".into()];

    let shown = format!("{spec:?}");

    for leaked in ["SECRETTOKEN", "PROXYSECRET", "pw", "abc"] {
        assert!(!shown.contains(leaked), "{leaked} in {shown}");
    }
}

#[test]
fn a_shown_url_has_no_userinfo_and_no_query_values() {
    let shown = masked_url(&url("wss://u:pw@x.test:8443/a?token=abc&n=1"));

    assert_eq!(shown, "wss://x.test:8443/a?token=***&n=***");
    assert_eq!(masked_url(&url("ws://x.test/a")), "ws://x.test/a");
}

#[test]
fn an_unresolved_variable_in_the_url_is_refused() {
    let resolved = Resolved {
        template_url: "wss://{{host}}/a".into(),
        unresolved: vec!["host".into()],
        ..Resolved::default()
    };
    let unrelated = Resolved {
        template_url: "wss://x/a".into(),
        unresolved: vec!["token".into()],
        ..Resolved::default()
    };

    let error = undefined_in_url(&resolved).unwrap();

    assert!(error.message.contains("`host`"), "{}", error.message);
    assert!(undefined_in_url(&unrelated).is_none());
}

#[test]
fn a_bare_spec_is_direct_and_verified() {
    let spec = WsConnectSpec::new(url("ws://x.test/"));

    assert!(spec.tls.verify);
    assert!(spec.proxy.is_none() && spec.headers.is_empty());
}
