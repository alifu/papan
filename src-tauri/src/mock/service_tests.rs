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

async fn start_at(
    root: &std::path::Path,
    mock: super::model::MockEnvironment,
) -> (MockService, String, Arc<Collect>) {
    let sink = Arc::new(Collect::default());
    let service = MockService::new(sink.clone());
    service
        .start("c1", "m.mock.yaml", root.to_path_buf(), mock)
        .await
        .unwrap();
    let base = service.url_of("c1", "m.mock.yaml").await.unwrap();
    (service, base, sink)
}

async fn start(mock: super::model::MockEnvironment) -> (MockService, String, Arc<Collect>) {
    start_at(std::path::Path::new("."), mock).await
}

async fn port_of(service: &MockService, file: &str) -> Option<u16> {
    let url = service.url_of("c1", file).await?;
    url.rsplit(':').next()?.parse().ok()
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
        file: String::new(),
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
        file: String::new(),
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
    let port = port_of(&service, "m.mock.yaml").await.unwrap();
    service.stop("c1", "m.mock.yaml").await;
    assert!(port_of(&service, "m.mock.yaml").await.is_none());
    assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_ok());
}

#[tokio::test]
async fn starting_twice_and_port_clashes_are_errors() {
    let (service, _, _) = start(sample()).await;
    assert!(service
        .start("c1", "m.mock.yaml", ".".into(), sample())
        .await
        .is_err());
    let port = port_of(&service, "m.mock.yaml").await.unwrap();
    let mut clash = sample();
    clash.port = port;
    let error = service
        .start("c1", "other.mock.yaml", ".".into(), clash)
        .await
        .unwrap_err();
    assert!(error.message.contains("in use"), "{}", error.message);
    service.stop_all().await;
}

fn response_with_file(path: &str) -> super::model::MockEnvironment {
    let mut mock = sample();
    mock.routes[0].responses[0].file = path.into();
    mock
}

#[tokio::test]
async fn a_file_response_sends_the_file_with_its_content_type() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("files")).unwrap();
    std::fs::write(dir.path().join("files/a.json"), b"{\"from\":\"file\"}").unwrap();
    let (service, base, _) = start_at(dir.path(), response_with_file("files/a.json")).await;
    let reply = reqwest::get(format!("{base}/users/1")).await.unwrap();
    assert_eq!(reply.status(), 200);
    assert_eq!(reply.headers()["content-type"], "application/json");
    assert_eq!(reply.text().await.unwrap(), "{\"from\":\"file\"}");
    service.stop_all().await;
}

#[tokio::test]
async fn a_missing_file_is_a_500_that_names_no_absolute_path() {
    let dir = tempfile::tempdir().unwrap();
    let (service, base, _) = start_at(dir.path(), response_with_file("files/none.json")).await;
    let reply = reqwest::get(format!("{base}/users/1")).await.unwrap();
    assert_eq!(reply.status(), 500);
    let text = reply.text().await.unwrap();
    assert!(
        text.contains("not found") && !text.contains(dir.path().to_str().unwrap()),
        "{text}"
    );
    service.stop_all().await;
}

#[tokio::test]
async fn https_mocks_answer_over_tls_and_report_an_https_url() {
    let mut mock = sample();
    mock.tls = true;
    let (service, base, _) = start(mock).await;
    assert!(base.starts_with("https://"), "{base}");
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();
    let reply = client.get(format!("{base}/users/1")).send().await.unwrap();
    assert_eq!(reply.status(), 200);
    // The certificate is self-signed, so a client that verifies refuses it.
    assert!(reqwest::get(format!("{base}/users/1")).await.is_err());
    service.stop_all().await;
}

#[tokio::test]
async fn unmatched_requests_are_forwarded_when_a_proxy_target_is_set() {
    let (upstream, upstream_base, _) = start(sample()).await;
    let mut front = sample();
    front.routes.clear();
    front.proxy = upstream_base.clone();
    let sink = Arc::new(Collect::default());
    let service = MockService::new(sink);
    service
        .start("c1", "front.mock.yaml", ".".into(), front)
        .await
        .unwrap();
    let base = service.url_of("c1", "front.mock.yaml").await.unwrap();
    let reply = reqwest::get(format!("{base}/users/5?x=1")).await.unwrap();
    assert_eq!(reply.status(), 200);
    assert_eq!(reply.text().await.unwrap(), "{}");
    let seen = upstream.log("c1", "m.mock.yaml").await;
    assert_eq!(
        (seen[0].path.as_str(), seen[0].query.as_str()),
        ("/users/5", "x=1")
    );
    // An unknown path is the upstream's 404, relayed.
    assert_eq!(
        reqwest::get(format!("{base}/nope")).await.unwrap().status(),
        404
    );
    service.stop_all().await;
    upstream.stop_all().await;
}

#[tokio::test]
async fn an_unreachable_proxy_target_is_a_502_and_a_self_proxy_is_refused() {
    let mut mock = sample();
    mock.routes.clear();
    mock.proxy = "http://127.0.0.1:1".into();
    let (service, base, _) = start(mock).await;
    assert_eq!(
        reqwest::get(format!("{base}/x")).await.unwrap().status(),
        502
    );
    let port = port_of(&service, "m.mock.yaml").await.unwrap();
    let mut looping = sample();
    looping.proxy = format!("http://localhost:{port}");
    looping.port = 0;
    // A fresh port is picked at start, so the loop guard is checked against the bound one.
    let error = super::server::refuse_own_port(&looping.proxy, port).unwrap_err();
    assert!(error.message.contains("itself"));
    service.stop_all().await;
}
