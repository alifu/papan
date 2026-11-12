use super::*;
use crate::error::ErrorCode;
use crate::store::types::Pair;

const VERSION: &str = "1.2.3";

fn content(url: &str) -> RequestContent {
    RequestContent::new("GET", url)
}

fn header(name: &str, value: &str, enabled: bool) -> KeyValue {
    KeyValue {
        name: name.into(),
        value: value.into(),
        enabled,
    }
}

fn sent_url(url: &str) -> String {
    build(&content(url), VERSION).unwrap().url.to_string()
}

fn headers_of(prepared: &Prepared) -> Vec<(String, String)> {
    prepared
        .headers
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_str().unwrap().to_string()))
        .collect()
}

fn header_of(prepared: &Prepared, name: &str) -> Option<String> {
    headers_of(prepared)
        .into_iter()
        .find(|(header, _)| header == name)
        .map(|(_, value)| value)
}

fn with_auth(auth: Auth) -> Prepared {
    let request = RequestContent {
        auth,
        ..content("https://x.test/a")
    };
    build(&request, VERSION).unwrap()
}

#[test]
fn a_missing_scheme_defaults_to_http_and_other_schemes_are_rejected() {
    assert_eq!(sent_url("example.com/a"), "http://example.com/a");
    assert_eq!(sent_url("localhost:8080/a"), "http://localhost:8080/a");
    assert_eq!(sent_url("HTTPS://example.com"), "https://example.com/");
    for url in [
        "file:///etc/passwd",
        "ftp://example.com",
        "javascript:alert(1)",
        "",
        "http://",
    ] {
        let error = build(&content(url), VERSION).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{url}");
    }
}

#[test]
fn the_query_is_sent_as_typed_with_minimal_escaping() {
    assert_eq!(
        sent_url("https://x.test/p?a=%20&b=%26&c=a+b&d={{var}}"),
        "https://x.test/p?a=%20&b=%26&c=a+b&d={{var}}"
    );
    assert_eq!(
        sent_url("https://x.test/p?q=a b&n=é"),
        "https://x.test/p?q=a%20b&n=%C3%A9"
    );
}

#[test]
fn disabled_params_are_never_sent() {
    let request = RequestContent {
        disabled_params: vec![Pair {
            name: "debug".into(),
            value: "1".into(),
        }],
        ..content("https://x.test/p?a=1")
    };

    let prepared = build(&request, VERSION).unwrap();

    assert_eq!(prepared.url.query(), Some("a=1"));
}

#[test]
fn api_key_in_query_is_appended_with_or_without_a_query() {
    let auth = || Auth::ApiKey {
        name: "api key".into(),
        value: "a&b".into(),
        placement: ApiKeyPlacement::Query,
    };
    let bare = build(
        &RequestContent {
            auth: auth(),
            ..content("https://x.test/p")
        },
        VERSION,
    )
    .unwrap();
    let with_query = build(
        &RequestContent {
            auth: auth(),
            ..content("https://x.test/p?a=1")
        },
        VERSION,
    )
    .unwrap();

    assert_eq!(bare.url.query(), Some("api+key=a%26b"));
    assert_eq!(with_query.url.query(), Some("a=1&api+key=a%26b"));
    assert_eq!(header_of(&bare, "authorization"), None);
}

#[test]
fn api_key_in_header_is_added() {
    let prepared = with_auth(Auth::ApiKey {
        name: "X-API-Key".into(),
        value: "k".into(),
        placement: ApiKeyPlacement::Header,
    });

    assert_eq!(header_of(&prepared, "x-api-key").as_deref(), Some("k"));
    assert_eq!(prepared.url.query(), None);
}

#[test]
fn user_headers_keep_order_and_duplicates_and_skip_disabled_ones() {
    let request = RequestContent {
        headers: vec![
            header("Accept", "a", true),
            header("X-Off", "1", false),
            header("Accept", "b", true),
            header("", "ignored", true),
        ],
        ..content("https://x.test")
    };

    let prepared = build(&request, VERSION).unwrap();

    assert_eq!(
        headers_of(&prepared),
        [
            ("accept".to_string(), "a".to_string()),
            ("accept".to_string(), "b".to_string()),
            ("user-agent".to_string(), "Papan/1.2.3".to_string()),
        ]
    );
}

#[test]
fn user_headers_beat_the_defaults() {
    let request = RequestContent {
        headers: vec![
            header("authorization", "Custom x", true),
            header("Content-Type", "application/vnd.x+json", true),
            header("User-Agent", "curl", true),
        ],
        body: Body::Json { text: "{}".into() },
        auth: Auth::Bearer { token: "t".into() },
        ..content("https://x.test")
    };

    let prepared = build(&request, VERSION).unwrap();

    assert_eq!(prepared.headers.len(), 3);
    assert_eq!(
        header_of(&prepared, "authorization").as_deref(),
        Some("Custom x")
    );
    assert_eq!(header_of(&prepared, "user-agent").as_deref(), Some("curl"));
}

#[test]
fn basic_auth_encodes_colons_and_non_ascii() {
    let prepared = with_auth(Auth::Basic {
        username: "a:b".into(),
        password: "pässword".into(),
    });
    let expected = base64::engine::general_purpose::STANDARD.encode("a:b:pässword");

    assert_eq!(
        header_of(&prepared, "authorization"),
        Some(format!("Basic {expected}"))
    );
}

#[test]
fn bearer_auth_sets_the_authorization_header() {
    let prepared = with_auth(Auth::Bearer { token: "t1".into() });

    assert_eq!(
        header_of(&prepared, "authorization").as_deref(),
        Some("Bearer t1")
    );
}

#[test]
fn bodies_set_their_content_type() {
    let cases = [
        (
            Body::Json { text: "{}".into() },
            Some(JSON_TYPE),
            b"{}".to_vec(),
        ),
        (
            Body::Xml {
                text: "<a/>".into(),
            },
            Some(XML_TYPE),
            b"<a/>".to_vec(),
        ),
        (
            Body::Text { text: "hi".into() },
            Some(TEXT_TYPE),
            b"hi".to_vec(),
        ),
        (
            Body::Form {
                fields: vec![
                    header("a b", "c&d", true),
                    header("off", "1", false),
                    header("e", "é", true),
                ],
            },
            Some(FORM_TYPE),
            b"a+b=c%26d&e=%C3%A9".to_vec(),
        ),
    ];
    for (body, content_type, bytes) in cases {
        let request = RequestContent {
            body,
            ..RequestContent::new("POST", "https://x.test")
        };
        let prepared = build(&request, VERSION).unwrap();
        assert_eq!(
            header_of(&prepared, "content-type").as_deref(),
            content_type
        );
        assert_eq!(prepared.body, PreparedBody::Bytes(bytes));
    }
}

#[test]
fn a_body_on_get_is_sent_as_given() {
    let request = RequestContent {
        body: Body::Text { text: "x".into() },
        ..content("https://x.test")
    };

    let prepared = build(&request, VERSION).unwrap();

    assert_eq!(prepared.method, Method::GET);
    assert_eq!(prepared.body, PreparedBody::Bytes(b"x".to_vec()));
}

#[test]
fn multipart_drops_a_user_content_type_and_lists_its_files() {
    let request = RequestContent {
        headers: vec![header("Content-Type", "text/plain", true)],
        body: Body::Multipart {
            parts: vec![
                Part {
                    name: "t".into(),
                    value: "v".into(),
                    file: None,
                    enabled: true,
                },
                Part {
                    name: "f".into(),
                    value: String::new(),
                    file: Some("/tmp/a.bin".into()),
                    enabled: true,
                },
                Part {
                    name: "off".into(),
                    value: String::new(),
                    file: Some("/tmp/b.bin".into()),
                    enabled: false,
                },
            ],
        },
        ..RequestContent::new("POST", "https://x.test")
    };

    let prepared = build(&request, VERSION).unwrap();

    assert_eq!(header_of(&prepared, "content-type"), None);
    assert_eq!(prepared.files(), [PathBuf::from("/tmp/a.bin")]);
    let PreparedBody::Multipart(parts) = &prepared.body else {
        panic!("not multipart")
    };
    assert_eq!(parts[0].value, PartValue::Text("v".into()));
}

#[test]
fn binary_body_needs_a_file() {
    let with_body = |body| RequestContent {
        body,
        ..RequestContent::new("PUT", "https://x.test")
    };

    let sent = build(
        &with_body(Body::Binary {
            file: Some("/tmp/x".into()),
        }),
        VERSION,
    )
    .unwrap();
    let missing = build(&with_body(Body::Binary { file: None }), VERSION).unwrap_err();

    assert_eq!(sent.files(), [PathBuf::from("/tmp/x")]);
    assert_eq!(missing.code, ErrorCode::InvalidInput);
}

#[test]
fn unsupported_sections_and_bad_headers_are_invalid_input() {
    let cases = [
        RequestContent {
            auth: Auth::Unsupported,
            ..content("https://x.test")
        },
        RequestContent {
            body: Body::Unsupported,
            ..content("https://x.test")
        },
        RequestContent {
            headers: vec![header("X-A", "a\r\nInjected: 1", true)],
            ..content("https://x.test")
        },
        RequestContent {
            headers: vec![header("Bad Name", "x", true)],
            ..content("https://x.test")
        },
        RequestContent::new("GE T", "https://x.test"),
    ];
    for request in cases {
        let error = build(&request, VERSION).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{request:?}");
    }
}

#[test]
fn logged_urls_drop_query_and_fragment() {
    let url = Url::parse("https://h.test:8443/a/b?key=secret#frag").unwrap();

    assert_eq!(loggable_url(&url), "https://h.test:8443/a/b");
}

#[test]
fn the_api_key_header_is_listed_as_secret() {
    let prepared = with_auth(Auth::ApiKey {
        name: "X-Key".into(),
        value: "k".into(),
        placement: ApiKeyPlacement::Header,
    });

    assert_eq!(prepared.secret_headers, [HeaderName::from_static("x-key")]);
}
