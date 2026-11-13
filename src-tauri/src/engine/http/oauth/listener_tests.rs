use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::*;
use crate::error::ErrorCode;

const STATE: &str = "expected-state-value";

fn request(target: &str, host: &str) -> Vec<u8> {
    format!("GET {target} HTTP/1.1\r\nHost: {host}\r\nAccept: */*\r\n\r\n").into_bytes()
}

fn code(text: &str) -> Verdict {
    Verdict::Done(Callback::Code(Zeroizing::new(text.to_string())))
}

/// Sends raw bytes to the listener and returns the whole reply.
async fn exchange(port: u16, bytes: &[u8]) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream.write_all(bytes).await.unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).await.unwrap();
    reply
}

#[test]
fn judge_accepts_the_callback_with_the_right_host_and_state() {
    let bytes = request(
        "/callback?code=abc%2B1&state=expected-state-value",
        "127.0.0.1:4000",
    );

    let verdict = judge(&bytes, 4000, STATE);

    assert_eq!(verdict, code("abc+1"));
}

#[test]
fn judge_answers_not_found_for_a_wrong_host_path_or_method() {
    let target = "/callback?code=abc&state=expected-state-value";
    let cases: Vec<Vec<u8>> = vec![
        request(target, "localhost:4000"),
        request(target, "127.0.0.1:4001"),
        request(target, "127.0.0.1"),
        request(target, "evil.example:4000"),
        request(
            "/other?code=abc&state=expected-state-value",
            "127.0.0.1:4000",
        ),
        request("/callback/x?state=expected-state-value", "127.0.0.1:4000"),
        request(
            "http://127.0.0.1:4000/callback?state=expected-state-value",
            "127.0.0.1:4000",
        ),
        format!("POST {target} HTTP/1.1\r\nHost: 127.0.0.1:4000\r\n\r\n").into_bytes(),
        format!("GET {target} HTTP/1.1\r\n\r\n").into_bytes(),
        format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1:4000\r\nHost: 127.0.0.1:4000\r\n\r\n")
            .into_bytes(),
        format!("GET {target} HTTP/2\r\nHost: 127.0.0.1:4000\r\n\r\n").into_bytes(),
        vec![0xff, 0xfe, b'\r', b'\n', b'\r', b'\n'],
        Vec::new(),
    ];

    for bytes in cases {
        assert_eq!(
            judge(&bytes, 4000, STATE),
            Verdict::NotFound,
            "{}",
            String::from_utf8_lossy(&bytes)
        );
    }
}

#[test]
fn judge_answers_bad_request_for_a_wrong_missing_or_repeated_state() {
    let targets = [
        "/callback?code=abc&state=wrong",
        "/callback?code=abc",
        "/callback?code=abc&state=",
        "/callback?code=abc&state=expected-state-value&state=expected-state-value",
        "/callback?code=abc&state=expected-state-valuE",
        "/callback?state=expected-state-value",
        "/callback?code=&state=expected-state-value",
        "/callback?error=access_denied&state=wrong",
    ];

    for target in targets {
        let verdict = judge(&request(target, "127.0.0.1:4000"), 4000, STATE);

        assert_eq!(verdict, Verdict::BadRequest, "{target}");
    }
}

#[test]
fn judge_reports_the_whole_provider_error_as_plain_text_for_the_flow_to_bound() {
    let long = "x".repeat(2000);
    let target = format!(
        "/callback?error=access_denied&error_description=No%0D%0Aaccess+{long}\
         &state=expected-state-value"
    );

    let verdict = judge(&request(&target, "127.0.0.1:4000"), 4000, STATE);

    let Verdict::Done(Callback::Denied(text)) = verdict else {
        panic!("expected a denial");
    };
    assert!(text.starts_with("access_denied No  access xxx"));
    assert!(text.ends_with(&"x".repeat(2000)));
    assert!(!text.chars().any(char::is_control));
}

#[test]
fn replies_are_fixed_pages_with_the_protective_headers() {
    let verdicts = [Verdict::NotFound, Verdict::BadRequest, code("secret-code")];

    for verdict in verdicts {
        let text = reply(&verdict);

        assert!(text.contains("Cache-Control: no-store\r\n"));
        assert!(text.contains("Referrer-Policy: no-referrer\r\n"));
        assert!(text.contains("Content-Security-Policy: default-src 'none'\r\n"));
        assert!(!text.contains("secret-code"));
    }
    assert!(reply(&code("c")).starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(reply(&code("c")).contains("You can close this tab."));
    assert!(reply(&Verdict::NotFound).starts_with("HTTP/1.1 404 "));
    assert!(reply(&Verdict::BadRequest).starts_with("HTTP/1.1 400 "));
}

#[tokio::test]
async fn binds_the_ipv4_loopback_address_only() {
    let listener = Listener::bind(0).await.unwrap();

    let address = listener.socket.local_addr().unwrap();

    assert_eq!(address.ip().to_string(), "127.0.0.1");
    assert_ne!(listener.port(), 0);
    assert_eq!(
        listener.redirect_uri(),
        format!("http://127.0.0.1:{}/callback", listener.port())
    );
}

#[tokio::test]
async fn a_busy_fixed_port_is_refused_naming_the_port() {
    let first = Listener::bind(0).await.unwrap();

    let error = Listener::bind(first.port()).await.unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(error.message.contains(&first.port().to_string()));
}

#[tokio::test]
async fn wrong_requests_get_an_error_page_and_the_wait_continues_until_the_real_callback() {
    let listener = Listener::bind(0).await.unwrap();
    let port = listener.port();
    let host = format!("127.0.0.1:{port}");
    let wait = tokio::spawn(async move { listener.wait(STATE).await });

    let wrong_state = exchange(
        port,
        &request("/callback?code=evil-code&state=guess", &host),
    )
    .await;
    let wrong_host = exchange(
        port,
        &request(
            "/callback?code=evil-code&state=expected-state-value",
            "localhost",
        ),
    )
    .await;
    let wrong_path = exchange(port, &request("/favicon.ico?q=marker-text", &host)).await;
    let oversized = exchange(port, &vec![b'a'; MAX_REQUEST_BYTES]).await;

    assert!(wrong_state.starts_with("HTTP/1.1 400 "));
    assert!(wrong_host.starts_with("HTTP/1.1 404 "));
    assert!(wrong_path.starts_with("HTTP/1.1 404 "));
    assert!(oversized.starts_with("HTTP/1.1 404 "));
    for reply in [&wrong_state, &wrong_host, &wrong_path] {
        assert!(!reply.contains("evil-code") && !reply.contains("marker-text"));
        assert!(!reply.contains("guess"));
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!wait.is_finished());

    let good = exchange(
        port,
        &request("/callback?code=real-code&state=expected-state-value", &host),
    )
    .await;

    assert!(good.starts_with("HTTP/1.1 200 OK"));
    assert!(good.contains("You can close this tab."));
    assert!(!good.contains("real-code") && !good.contains(STATE));
    let callback = wait.await.unwrap();
    assert_eq!(
        callback,
        Callback::Code(Zeroizing::new("real-code".to_string()))
    );
}

#[tokio::test]
async fn a_stalled_connection_does_not_block_the_real_callback() {
    let listener = Listener::bind(0).await.unwrap();
    let port = listener.port();
    let wait = tokio::spawn(async move { listener.wait(STATE).await });
    let mut stalled = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stalled.write_all(b"GET /callback").await.unwrap();

    let good = exchange(
        port,
        &request(
            "/callback?error=access_denied&state=expected-state-value",
            &format!("127.0.0.1:{port}"),
        ),
    )
    .await;

    assert!(good.starts_with("HTTP/1.1 200 OK"));
    let callback = wait.await.unwrap();
    assert_eq!(callback, Callback::Denied("access_denied".to_string()));
}

#[tokio::test]
async fn the_port_is_closed_once_the_wait_is_dropped() {
    let listener = Listener::bind(0).await.unwrap();
    let port = listener.port();
    let wait = tokio::spawn(async move { listener.wait(STATE).await });

    wait.abort();
    let _ = wait.await;

    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
}
