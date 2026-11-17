use super::*;
use crate::error::ErrorCode;
use crate::store::types::{RequestKind, WsSettings};

const VERSION: &str = "1.2.3";

fn ws(url: &str) -> RequestContent {
    RequestContent {
        kind: RequestKind::Ws,
        ..RequestContent::new("GET", url)
    }
}

fn header_of(parts: &HandshakeParts, name: &str) -> Option<String> {
    parts
        .headers
        .iter()
        .find(|(header, _)| header == name)
        .map(|(_, value)| value.to_str().unwrap().to_string())
}

fn header(name: &str, value: &str) -> KeyValue {
    KeyValue {
        name: name.into(),
        value: value.into(),
        enabled: true,
    }
}

#[test]
fn only_ws_and_wss_urls_are_accepted() {
    for (text, expected) in [
        ("ws://x.test/a", "ws://x.test/a"),
        ("WSS://x.test:8443/a?b=1", "wss://x.test:8443/a?b=1"),
    ] {
        let parts = handshake_parts(&ws(text), VERSION).unwrap();
        assert_eq!(parts.url.as_str(), expected);
    }
    for text in [
        "",
        "x.test",
        "http://x.test",
        "https://x.test",
        "ws://",
        "ftp://x",
    ] {
        let error = handshake_parts(&ws(text), VERSION).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{text}");
        assert!(error.message.contains("ws://"), "{}", error.message);
    }
}

#[test]
fn auth_kinds_become_headers_or_query() {
    let bearer = RequestContent {
        auth: Auth::Bearer {
            token: "t0k".into(),
        },
        ..ws("wss://x.test/a")
    };
    let basic = RequestContent {
        auth: Auth::Basic {
            username: "u".into(),
            password: "p".into(),
        },
        ..ws("wss://x.test/a")
    };
    let key_header = RequestContent {
        auth: Auth::ApiKey {
            name: "X-Key".into(),
            value: "k1".into(),
            placement: ApiKeyPlacement::Header,
        },
        ..ws("wss://x.test/a")
    };
    let key_query = RequestContent {
        auth: Auth::ApiKey {
            name: "key".into(),
            value: "a b".into(),
            placement: ApiKeyPlacement::Query,
        },
        ..ws("wss://x.test/a?z=1")
    };

    let parts = |content| handshake_parts(&content, VERSION).unwrap();

    assert_eq!(
        header_of(&parts(bearer), "authorization").as_deref(),
        Some("Bearer t0k")
    );
    assert_eq!(
        header_of(&parts(basic), "authorization").as_deref(),
        Some("Basic dTpw")
    );
    let by_header = parts(key_header);
    assert_eq!(header_of(&by_header, "x-key").as_deref(), Some("k1"));
    assert_eq!(by_header.secret_headers.len(), 1);
    assert_eq!(parts(key_query).url.query(), Some("z=1&key=a+b"));
}

#[test]
fn a_user_header_wins_over_the_default() {
    let content = RequestContent {
        headers: vec![header("Authorization", "mine"), header("User-Agent", "me")],
        auth: Auth::Bearer { token: "t".into() },
        ..ws("ws://x.test")
    };

    let parts = handshake_parts(&content, VERSION).unwrap();

    assert_eq!(header_of(&parts, "authorization").as_deref(), Some("mine"));
    assert_eq!(header_of(&parts, "user-agent").as_deref(), Some("me"));
}

#[test]
fn default_user_agent_and_subprotocols_are_added() {
    let content = RequestContent {
        ws: WsSettings {
            subprotocols: vec!["chat.v2".into(), "graphql-transport-ws".into()],
            messages: Vec::new(),
        },
        ..ws("ws://x.test")
    };

    let parts = handshake_parts(&content, VERSION).unwrap();

    assert_eq!(
        header_of(&parts, "user-agent").as_deref(),
        Some("Papan/1.2.3")
    );
    assert_eq!(
        header_of(&parts, "sec-websocket-protocol").as_deref(),
        Some("chat.v2, graphql-transport-ws")
    );
    assert_eq!(parts.subprotocols, ["chat.v2", "graphql-transport-ws"]);
}

#[test]
fn a_user_subprotocol_header_wins_over_the_section() {
    let content = RequestContent {
        headers: vec![header("Sec-WebSocket-Protocol", "mine")],
        ws: WsSettings {
            subprotocols: vec!["chat".into()],
            messages: Vec::new(),
        },
        ..ws("ws://x.test")
    };

    let parts = handshake_parts(&content, VERSION).unwrap();

    assert_eq!(
        header_of(&parts, "sec-websocket-protocol").as_deref(),
        Some("mine")
    );
}

#[test]
fn digest_and_unsupported_auth_are_refused() {
    let digest = RequestContent {
        auth: Auth::Digest {
            username: "u".into(),
            password: "p".into(),
        },
        ..ws("ws://x.test")
    };
    let error = handshake_parts(&digest, VERSION).unwrap_err();
    assert_eq!(error.message, "Digest is not supported for WebSocket.");

    let unsupported = RequestContent {
        auth: Auth::Unsupported,
        ..ws("ws://x.test")
    };
    assert!(handshake_parts(&unsupported, VERSION).is_err());
}

#[test]
fn bad_header_names_are_refused() {
    let content = RequestContent {
        headers: vec![header("bad name", "v")],
        ..ws("ws://x.test")
    };

    assert!(handshake_parts(&content, VERSION).is_err());
}
