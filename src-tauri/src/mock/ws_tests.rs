//! End-to-end: a WebSocket client against a running mock.

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::WebSocketStream;

use super::model::tests_support::sample;
use super::model::{
    default_id_key, MockEnvironment, MockResponse, MockRoute, ResponseMode, RouteKind, Rule,
    RuleOperator, RuleTarget, RulesOperator,
};
use super::service_tests::start;

fn reply(id: &str, body: &str, rules: Vec<Rule>, default: bool) -> MockResponse {
    MockResponse {
        id: id.into(),
        label: String::new(),
        status: 200,
        headers: vec![],
        body: body.into(),
        latency_ms: 0,
        rules,
        rules_operator: RulesOperator::And,
        default,
        file: String::new(),
        callbacks: vec![],
    }
}

fn with_chat() -> MockEnvironment {
    let mut mock = sample();
    mock.routes.push(MockRoute {
        id: "ws1".into(),
        method: "get".into(),
        endpoint: "chat/:room".into(),
        enabled: true,
        mode: ResponseMode::Rules,
        kind: RouteKind::Ws,
        bucket: String::new(),
        id_key: default_id_key(),
        greeting: "hello {{urlParam 'room'}}".into(),
        responses: vec![
            reply(
                "ping",
                "pong",
                vec![Rule {
                    target: RuleTarget::Body,
                    modifier: String::new(),
                    operator: RuleOperator::Equals,
                    value: "ping".into(),
                    invert: false,
                }],
                false,
            ),
            reply("echo", "echo: {{body}}", vec![], true),
        ],
    });
    mock
}

async fn connect(
    base: &str,
    path: &str,
    origin: Option<&str>,
) -> Result<WebSocketStream<TcpStream>, tokio_tungstenite::tungstenite::Error> {
    let host = base.trim_start_matches("http://");
    let stream = TcpStream::connect(host).await.unwrap();
    let mut request = format!("ws://{host}{path}").into_client_request().unwrap();
    if let Some(origin) = origin {
        request
            .headers_mut()
            .insert("Origin", origin.parse().unwrap());
    }
    tokio_tungstenite::client_async(request, stream)
        .await
        .map(|(ws, _)| ws)
}

async fn next_text(ws: &mut WebSocketStream<TcpStream>) -> String {
    match tokio::time::timeout(std::time::Duration::from_secs(5), ws.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => text.to_string(),
        other => panic!("expected a text message, got {other:?}"),
    }
}

#[tokio::test]
async fn a_websocket_route_greets_and_answers_by_rule() {
    let (service, base, _) = start(with_chat()).await;
    let mut ws = connect(&base, "/chat/lobby", None).await.unwrap();
    assert_eq!(next_text(&mut ws).await, "hello lobby");
    ws.send(Message::text("ping")).await.unwrap();
    assert_eq!(next_text(&mut ws).await, "pong");
    ws.send(Message::text("anything")).await.unwrap();
    assert_eq!(next_text(&mut ws).await, "echo: anything");

    let log = service.log("c1", "m.mock.yaml").await;
    let shape: Vec<_> = log
        .iter()
        .map(|e| (e.method.as_str(), e.status, e.body_text.as_str()))
        .collect();
    assert_eq!(
        shape,
        [
            ("GET", 101, ""),
            ("WS", 200, "ping"),
            ("WS", 200, "anything")
        ]
    );
    assert_eq!(log[1].response_id.as_deref(), Some("ping"));
    service.stop_all().await;
}

#[tokio::test]
async fn plain_requests_and_other_paths_do_not_reach_a_websocket_route() {
    let (service, base, _) = start(with_chat()).await;
    assert_eq!(
        reqwest::get(format!("{base}/chat/lobby"))
            .await
            .unwrap()
            .status(),
        404
    );
    assert!(connect(&base, "/other", None).await.is_err());
    // A websocket route never answers an ordinary HTTP route's path either.
    assert!(connect(&base, "/users/1", None).await.is_err());
    service.stop_all().await;
}

#[tokio::test]
async fn a_mock_without_cors_refuses_foreign_origins() {
    let mut mock = with_chat();
    mock.cors = false;
    let (service, base, _) = start(mock).await;
    assert!(connect(&base, "/chat/a", Some("https://evil.example"))
        .await
        .is_err());
    assert!(connect(&base, "/chat/a", None).await.is_ok());
    let host = base.trim_start_matches("http://");
    assert!(connect(&base, "/chat/a", Some(&format!("http://{host}")))
        .await
        .is_ok());
    service.stop_all().await;
}

#[tokio::test]
async fn foreign_sites_are_refused_even_with_cors_but_local_pages_pass() {
    let (service, base, _) = start(with_chat()).await;
    assert!(connect(&base, "/chat/a", Some("https://evil.example"))
        .await
        .is_err());
    assert!(connect(&base, "/chat/a", Some("null")).await.is_err());
    assert!(connect(&base, "/chat/a", Some("http://localhost:5173"))
        .await
        .is_ok());
    service.stop_all().await;
}

#[tokio::test]
async fn a_repeated_or_unreadable_origin_is_refused() {
    let (service, base, _) = start(with_chat()).await;
    let host = base.trim_start_matches("http://");
    for headers in [
        "Origin: http://localhost\r\nOrigin: http://localhost\r\n".to_string(),
        "Origin: caf\u{e9}\r\n".to_string(),
    ] {
        let mut stream = TcpStream::connect(host).await.unwrap();
        let request = format!(
            "GET /chat/a HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n{headers}Sec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
        );
        tokio::io::AsyncWriteExt::write_all(&mut stream, request.as_bytes())
            .await
            .unwrap();
        let mut buf = vec![0_u8; 64];
        let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf)
            .await
            .unwrap();
        assert!(
            String::from_utf8_lossy(&buf[..n]).contains("403"),
            "{headers}"
        );
    }
    service.stop_all().await;
}

#[tokio::test]
async fn sequential_replies_count_per_connection_and_stop_closes_sessions() {
    let mut mock = with_chat();
    let route = &mut mock.routes[1];
    route.mode = ResponseMode::Sequential;
    route.greeting = String::new();
    route.responses = vec![
        reply("a", "one", vec![], false),
        reply("b", "two", vec![], false),
    ];
    let (service, base, _) = start(mock).await;
    for _ in 0..2 {
        let mut ws = connect(&base, "/chat/x", None).await.unwrap();
        ws.send(Message::text("m")).await.unwrap();
        assert_eq!(next_text(&mut ws).await, "one");
        ws.send(Message::text("m")).await.unwrap();
        assert_eq!(next_text(&mut ws).await, "two");
    }
    let mut ws = connect(&base, "/chat/x", None).await.unwrap();
    service.stop_all().await;
    let ended = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
        .await
        .expect("the session closes when the mock stops");
    assert!(!matches!(ended, Some(Ok(Message::Text(_)))));
}
