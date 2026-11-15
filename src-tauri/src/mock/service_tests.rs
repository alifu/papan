//! End-to-end: a real server on a free port, called with a real HTTP client.

use std::sync::{Arc, Mutex};

use super::model::tests_support::sample;
use super::model::{MockResponse, ResponseMode, Rule, RuleOperator, RuleTarget};
use super::types::MockLogEntry;
use super::{MockService, MockSink};

#[derive(Default)]
struct Collect(Mutex<Vec<u32>>);

impl MockSink for Collect {
    fn logged(&self, _: &str, _: &str, entry: &MockLogEntry) {
        self.0.lock().unwrap().push(entry.id);
    }
}

async fn start(mock: super::model::MockEnvironment) -> (MockService, String, Arc<Collect>) {
    let sink = Arc::new(Collect::default());
    let service = MockService::new(sink.clone());
    let port = service.start("c1", "m.mock.yaml", mock).await.unwrap();
    (service, format!("http://127.0.0.1:{port}"), sink)
}

#[tokio::test]
async fn serves_templated_json_with_cors_and_logs_the_request() {
    let mut mock = sample();
    mock.routes[0].responses[0].body =
        r#"{"id": "{{urlParam 'id'}}", "q": "{{queryParam 'q'}}"}"#.into();
    let (service, base, sink) = start(mock).await;

    let response = reqwest::get(format!("{base}/users/7?q=hi")).await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["access-control-allow-origin"], "*");
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(response.text().await.unwrap(), r#"{"id": "7", "q": "hi"}"#);

    let log = service.log("c1", "m.mock.yaml").await;
    assert_eq!(log.len(), 1);
    assert_eq!(
        (
            log[0].method.as_str(),
            log[0].path.as_str(),
            log[0].query.as_str()
        ),
        ("GET", "/users/7", "q=hi")
    );
    assert_eq!(log[0].status, 200);
    assert_eq!(log[0].route_id.as_deref(), Some("r1"));
    assert_eq!(sink.0.lock().unwrap().as_slice(), &[1]);
    service.clear_log("c1", "m.mock.yaml").await;
    assert!(service.log("c1", "m.mock.yaml").await.is_empty());
    service.stop_all().await;
}

#[tokio::test]
async fn unknown_paths_404_and_preflight_is_answered() {
    let (service, base, _) = start(sample()).await;
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .get(format!("{base}/nope"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    let preflight = client
        .request(reqwest::Method::OPTIONS, format!("{base}/users/1"))
        .send()
        .await
        .unwrap();
    assert_eq!(preflight.status(), 204);
    service.stop_all().await;
}

#[tokio::test]
async fn rules_read_the_request_body_and_sequential_cycles() {
    let mut mock = sample();
    mock.routes[0].method = "post".into();
    let matched = MockResponse {
        id: "ok".into(),
        status: 201,
        default: false,
        rules: vec![Rule {
            target: RuleTarget::Body,
            modifier: "user.role".into(),
            operator: RuleOperator::Equals,
            value: "admin".into(),
            invert: false,
        }],
        ..mock.routes[0].responses[0].clone()
    };
    mock.routes[0].responses.insert(0, matched);
    let (service, base, _) = start(mock).await;
    let client = reqwest::Client::new();
    let admin = client
        .post(format!("{base}/users/1"))
        .header("content-type", "application/json")
        .body(r#"{"user": {"role": "admin"}}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(admin.status(), 201);
    let other = client
        .post(format!("{base}/users/1"))
        .header("content-type", "application/json")
        .body(r#"{"user": {"role": "x"}}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(other.status(), 200);
    service.stop_all().await;

    let mut seq = sample();
    seq.routes[0].mode = ResponseMode::Sequential;
    let base_response = seq.routes[0].responses[0].clone();
    seq.routes[0].responses.push(MockResponse {
        id: "b".into(),
        status: 202,
        default: false,
        ..base_response
    });
    let (service, base, _) = start(seq).await;
    let mut statuses = Vec::new();
    for _ in 0..3 {
        statuses.push(
            reqwest::get(format!("{base}/users/1"))
                .await
                .unwrap()
                .status()
                .as_u16(),
        );
    }
    assert_eq!(statuses, [200, 202, 200]);
    service.stop_all().await;
}

#[tokio::test]
async fn latency_delays_the_reply() {
    let mut mock = sample();
    mock.latency_ms = 150;
    let (service, base, _) = start(mock).await;
    let started = std::time::Instant::now();
    reqwest::get(format!("{base}/users/1")).await.unwrap();
    assert!(started.elapsed() >= std::time::Duration::from_millis(140));
    service.stop_all().await;
}

#[tokio::test]
async fn oversized_bodies_are_refused_and_stop_frees_the_port() {
    let (service, base, _) = start(sample()).await;
    let huge = vec![b'a'; super::server::MAX_REQUEST_BODY + 10];
    let reply = reqwest::Client::new()
        .post(format!("{base}/users/1"))
        .body(huge)
        .send()
        .await
        .unwrap();
    assert_eq!(reply.status(), 413);
    let port = service.port_of("c1", "m.mock.yaml").await.unwrap();
    service.stop("c1", "m.mock.yaml").await;
    assert!(service.port_of("c1", "m.mock.yaml").await.is_none());
    assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_ok());
}

#[tokio::test]
async fn starting_twice_and_port_clashes_are_errors() {
    let (service, _, _) = start(sample()).await;
    assert!(service.start("c1", "m.mock.yaml", sample()).await.is_err());
    let port = service.port_of("c1", "m.mock.yaml").await.unwrap();
    let mut clash = sample();
    clash.port = port;
    let error = service
        .start("c1", "other.mock.yaml", clash)
        .await
        .unwrap_err();
    assert!(error.message.contains("in use"), "{}", error.message);
    service.stop_all().await;
}
