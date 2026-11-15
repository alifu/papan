use super::*;

fn settings(mode: ProxyMode, url: &str, bypass: &[&str]) -> ProxySettings {
    ProxySettings {
        mode,
        url: url.into(),
        username: String::new(),
        has_password: false,
        bypass: bypass.iter().map(|p| p.to_string()).collect(),
    }
}

fn target(text: &str) -> Url {
    Url::parse(text).unwrap()
}

#[test]
fn connect_request_names_the_target_and_carries_proxy_auth() {
    let plain = connect_request("example.com:443", None);
    let authed = connect_request("example.com:443", Some("Basic dTpw"));

    assert_eq!(
        plain,
        "CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n"
    );
    assert!(authed.contains("Proxy-Authorization: Basic dTpw\r\n"));
    assert!(authed.ends_with("\r\n\r\n"));
}

#[test]
fn a_custom_http_proxy_is_used_with_basic_auth() {
    let custom = ProxySettings {
        username: "alifu".into(),
        ..settings(ProxyMode::Custom, "http://proxy.test:3128", &[])
    };

    let hop = proxy_for(&custom, Some("pw"), &target("wss://api.test/x"))
        .unwrap()
        .unwrap();

    assert_eq!((hop.host.as_str(), hop.port), ("proxy.test", 3128));
    assert_eq!(hop.authorization.as_deref(), Some("Basic YWxpZnU6cHc="));
}

#[test]
fn none_mode_and_the_bypass_list_go_direct() {
    let none = settings(ProxyMode::None, "", &[]);
    let bypass = settings(ProxyMode::Custom, "http://proxy.test:3128", &["api.test"]);

    assert_eq!(
        proxy_for(&none, None, &target("ws://x.test")).unwrap(),
        None
    );
    assert_eq!(
        proxy_for(&bypass, None, &target("wss://api.test/x")).unwrap(),
        None
    );
    assert_eq!(
        proxy_for(&bypass, None, &target("wss://sub.api.test/x")).unwrap(),
        None
    );
    assert!(proxy_for(&bypass, None, &target("wss://other.test/x"))
        .unwrap()
        .is_some());
}

#[test]
fn an_https_proxy_is_refused_with_a_clear_message() {
    let https = settings(ProxyMode::Custom, "https://proxy.test:3128", &[]);

    let error = proxy_for(&https, None, &target("ws://x.test")).unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(error.message.contains("https://"), "{}", error.message);
}

#[tokio::test]
async fn a_non_200_connect_answer_is_an_error_with_the_status() {
    let reply: &[u8] = b"HTTP/1.1 407 Proxy Authentication Required\r\nX: y\r\n\r\n";
    let mut stream = reply;

    let (status, line) = read_connect_status(&mut stream).await.unwrap();

    assert_eq!(status, 407);
    assert!(line.contains("407"));
}

#[tokio::test]
async fn an_oversized_or_cut_connect_reply_is_an_error() {
    let mut cut: &[u8] = b"HTTP/1.1 200 OK\r\n";
    assert!(read_connect_status(&mut cut).await.is_err());
    let big = vec![b'a'; MAX_CONNECT_HEAD_BYTES + 10];
    let mut stream = &big[..];
    assert!(read_connect_status(&mut stream).await.is_err());
}

#[test]
fn tls_config_builds_for_both_verify_modes_and_rejects_bad_identities() {
    let verify = TlsOptions {
        verify: true,
        identity: None,
    };
    let lax = TlsOptions {
        verify: false,
        identity: None,
    };
    let broken = TlsOptions {
        verify: true,
        identity: Some(ClientIdentity(Zeroizing::new("not pem".into()))),
    };

    assert!(tls_config(&verify).is_ok());
    assert!(tls_config(&lax).is_ok());
    assert_eq!(
        tls_config(&broken).unwrap_err().code,
        ErrorCode::InvalidInput
    );
}
