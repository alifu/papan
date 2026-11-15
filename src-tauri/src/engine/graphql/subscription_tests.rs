use super::*;
use crate::engine::http::resolve::resolve_content;
use crate::error::ErrorCode;
use crate::store::types::{RequestKind, SubscriptionSettings};
use crate::vars::dynamic::FixedDynamics;
use crate::vars::scopes::Scopes;

fn content(url: &str, override_url: &str) -> RequestContent {
    RequestContent {
        kind: RequestKind::Graphql,
        subscription: SubscriptionSettings {
            url: override_url.into(),
            init_payload: String::new(),
        },
        ..RequestContent::new("POST", url)
    }
}

#[test]
fn ws_url_table() {
    let cases = [
        ("http://h/g", "", "ws://h/g"),
        ("https://h/g?a=1", "", "wss://h/g?a=1"),
        ("ws://h/g", "", "ws://h/g"),
        ("wss://h/g", "", "wss://h/g"),
        ("https://h/g", "wss://other/sub", "wss://other/sub"),
        ("https://h/g", "ws://other/sub", "ws://other/sub"),
    ];
    for (url, override_url, expected) in cases {
        let derived = ws_url(&content(url, override_url)).unwrap();
        assert_eq!(derived.as_str(), expected, "{url} {override_url}");
    }
}

#[test]
fn ws_url_refuses_other_schemes_and_junk() {
    for (url, override_url) in [
        ("ftp://h/g", ""),
        ("https://h", "ftp://x"),
        ("nonsense", ""),
    ] {
        let error = ws_url(&content(url, override_url)).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput);
        assert!(error.message.contains("http, https, ws or wss"));
    }
}

#[test]
fn frames_embed_the_query_and_check_the_init_payload() {
    let mut request = content("https://h/g", "");
    request.body = Body::Graphql {
        query: "subscription { a }".into(),
        variables: "{\"n\": 1}".into(),
        operation_name: "Op".into(),
    };
    request.subscription.init_payload = " {\"t\": 1} ".into();

    let built = frames(&Resolved::unchanged(request.clone())).unwrap();

    assert_eq!(built.init_payload.as_deref(), Some("{\"t\": 1}"));
    assert!(built.body_json.contains("\"operationName\":\"Op\""));
    request.subscription.init_payload = "[1]".into();
    assert!(frames(&Resolved::unchanged(request)).is_err());
}

#[test]
fn an_undefined_variable_in_the_init_payload_is_an_error() {
    let mut typed = content("https://h/g", "");
    typed.body = Body::Graphql {
        query: "subscription { a }".into(),
        variables: String::new(),
        operation_name: String::new(),
    };
    typed.subscription.init_payload = "{\"t\": \"{{missing}}\"}".into();
    let resolved = resolve_content(&typed, &Scopes::new(vec![]), &FixedDynamics::default());

    let error = check_defined(&typed, &resolved).unwrap_err();

    assert!(error.message.contains("missing"), "{}", error.message);
}
