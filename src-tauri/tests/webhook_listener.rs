//! The webhook listener over real sockets (plan 07 §10).

mod support;

use std::time::Duration;

use papan_lib::store::types::KeyValue;
use support::webhook::{config, running, url_of, world};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn port_of(url: &str) -> u16 {
    url::Url::parse(url).unwrap().port().unwrap()
}

#[tokio::test]
async fn captures_request_and_emits_event() {
    let w = world();
    let l = running(&w, config("/hooks")).await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/x?a=1", url_of(&l)))
        .header("X-Signature", "sig123")
        .body("{\"k\":1}")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let id = &l.config.id;
    let list = w.service.captures(id).unwrap();
    assert_eq!(list.len(), 1);
    let capture = w.service.read_capture(id, &list[0].id).unwrap();
    assert_eq!(capture.summary.method, "POST");
    assert_eq!(capture.summary.path, "/hooks/x");
    assert_eq!(capture.query, "a=1");
    assert_eq!(capture.body_text.as_deref(), Some("{\"k\":1}"));
    assert!(capture.summary.at_ms > 1.0e12);
    assert!(capture
        .headers
        .iter()
        .any(|h| h.name == "x-signature" && h.value == "sig123"));
    assert!(capture.remote.starts_with("127.0.0.1:"));
    let events = w.events.0.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, *id);
    assert_eq!(events[0].1.id, list[0].id);
    assert_eq!(w.service.list()[0].captured, 1);
}

#[tokio::test]
async fn replies_with_the_configured_response() {
    let w = world();
    let mut cfg = config("/hooks");
    cfg.response.status = 201;
    cfg.response.body = "created".into();
    cfg.response.headers = vec![KeyValue {
        name: "X-Custom".into(),
        value: "yes".into(),
        enabled: true,
    }];
    let l = running(&w, cfg).await;
    let response = reqwest::get(url_of(&l)).await.unwrap();
    assert_eq!(response.status(), 201);
    assert_eq!(response.headers()["x-custom"], "yes");
    assert!(response
        .headers()
        .get("access-control-allow-origin")
        .is_none());
    assert_eq!(response.text().await.unwrap(), "created");
}

#[tokio::test]
async fn other_paths_get_404_and_count_as_unmatched() {
    let w = world();
    let l = running(&w, config("/hooks")).await;
    let base = format!("http://127.0.0.1:{}", port_of(&url_of(&l)));
    for path in ["/other", "/hooksx"] {
        assert_eq!(
            reqwest::get(format!("{base}{path}"))
                .await
                .unwrap()
                .status(),
            404
        );
    }
    assert_eq!(
        reqwest::get(format!("{base}/hooks/deep"))
            .await
            .unwrap()
            .status(),
        200
    );
    let listed = &w.service.list()[0];
    assert_eq!((listed.captured, listed.unmatched), (1, 2));
}

#[tokio::test]
async fn body_over_the_cap_is_413_and_truncated() {
    let w = world();
    let mut cfg = config("/hooks");
    cfg.max_body_bytes = 1024;
    let l = running(&w, cfg).await;
    let client = reqwest::Client::new();
    let response = client
        .post(url_of(&l))
        .body(vec![b'a'; 4096])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 413);
    let id = &l.config.id;
    let summary = &w.service.captures(id).unwrap()[0];
    let capture = w.service.read_capture(id, &summary.id).unwrap();
    assert!(capture.truncated);
    assert!(capture.body_text.is_none() && capture.body_base64.is_none());
    assert!(!capture.headers.is_empty());
    let small = client.post(url_of(&l)).body("ok").send().await.unwrap();
    assert_eq!(small.status(), 200);
}

#[tokio::test]
async fn ring_keeps_only_max_captures() {
    let w = world();
    let mut cfg = config("/hooks");
    cfg.max_captures = 3;
    let l = running(&w, cfg).await;
    let client = reqwest::Client::new();
    for i in 0..5 {
        client
            .post(url_of(&l))
            .body(format!("n{i}"))
            .send()
            .await
            .unwrap();
    }
    let id = &l.config.id;
    let list = w.service.captures(id).unwrap();
    assert_eq!(list.len(), 3);
    let newest = w.service.read_capture(id, &list[0].id).unwrap();
    assert_eq!(newest.body_text.as_deref(), Some("n4"));
    w.service.clear_captures(id).unwrap();
    assert!(w.service.captures(id).unwrap().is_empty());
}

#[tokio::test]
async fn port_in_use_names_the_port() {
    let held = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = u32::from(held.local_addr().unwrap().port());
    let w = world();
    let mut cfg = config("/hooks");
    cfg.port = port.max(1024);
    if port < 1024 {
        return;
    }
    let saved = w.service.save(cfg).await.unwrap();
    let error = w.service.start(&saved.config.id).await.unwrap_err();
    assert_eq!(
        error.message,
        format!("Port {port} is already in use. Choose another port, or 0 for any free port.")
    );
}

#[tokio::test]
async fn another_papan_listener_on_the_same_port_is_refused() {
    let w = world();
    let first = running(&w, config("/a")).await;
    let port = u32::from(port_of(&url_of(&first)));
    let mut second = config("/b");
    second.port = port;
    let saved = w.service.save(second).await.unwrap();
    let error = w.service.start(&saved.config.id).await.unwrap_err();
    assert!(error.message.contains("another Papan listener"));
}

#[tokio::test]
async fn loopback_bind_and_stop_frees_the_port() {
    let w = world();
    let l = running(&w, config("/hooks")).await;
    let url = url_of(&l);
    assert!(url.starts_with("http://127.0.0.1:"));
    let port = port_of(&url);
    assert!(l.running);
    w.service.stop(&l.config.id).await.unwrap();
    assert!(!w.service.list()[0].running);
    assert!(w.service.list()[0].url.is_none());
    assert!(tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .is_ok());
}

#[tokio::test]
async fn slow_header_connection_is_closed() {
    let w = world();
    let l = running(&w, config("/hooks")).await;
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port_of(&url_of(&l))))
        .await
        .unwrap();
    stream
        .write_all(b"POST /hooks HTTP/1.1\r\nHost: x\r\n")
        .await
        .unwrap();
    let mut buffer = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(15), stream.read_to_end(&mut buffer)).await;
    assert!(read.is_ok(), "the server should close the connection");
}

#[tokio::test(flavor = "multi_thread")]
async fn thirty_two_concurrent_posts_are_all_captured() {
    let w = world();
    let l = running(&w, config("/hooks")).await;
    let client = reqwest::Client::new();
    let url = url_of(&l);
    let mut tasks = Vec::new();
    for i in 0..32 {
        let (client, url) = (client.clone(), url.clone());
        tasks.push(tokio::spawn(async move {
            client
                .post(url)
                .body(format!("p{i}"))
                .send()
                .await
                .unwrap()
                .status()
        }));
    }
    for task in tasks {
        assert_eq!(task.await.unwrap(), 200);
    }
    assert_eq!(w.service.captures(&l.config.id).unwrap().len(), 32);
}

#[tokio::test]
async fn saving_a_running_listener_restarts_it_and_config_persists() {
    let w = world();
    let l = running(&w, config("/hooks")).await;
    let mut edited = l.config.clone();
    edited.path = "/renamed".into();
    let after = w.service.save(edited).await.unwrap();
    assert!(after.running);
    assert!(url_of(&after).ends_with("/renamed"));
    w.service.stop_all().await;
    let reloaded = papan_lib::webhook::WebhookService::new(w.dir.path(), w.events.clone());
    assert_eq!(reloaded.list()[0].config.path, "/renamed");
    assert!(!reloaded.list()[0].running);
    w.service.delete(&l.config.id).await.unwrap();
    assert!(w.service.list().is_empty());
}

#[tokio::test]
async fn a_failed_restart_on_save_reports_stopped_and_leaves_it_not_running() {
    let held = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let busy = u32::from(held.local_addr().unwrap().port());
    let w = world();
    let l = running(&w, config("/hooks")).await;
    let mut edited = l.config.clone();
    edited.port = busy;

    let result = w.service.save(edited).await;

    assert!(result.is_err());
    assert!(!w.service.list()[0].running);
    let stopped = w.events.1.lock().unwrap();
    assert_eq!(stopped.len(), 1);
    assert_eq!(stopped[0].0, l.config.id);
    assert!(stopped[0].1.is_some());
}

#[tokio::test]
async fn a_chunked_body_over_the_cap_records_the_bytes_seen() {
    let w = world();
    let mut cfg = config("/hooks");
    cfg.max_body_bytes = 1024;
    let l = running(&w, cfg).await;
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port_of(&url_of(&l))))
        .await
        .unwrap();
    let chunk = "a".repeat(2048);
    let request = format!(
        "POST /hooks HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n800\r\n{chunk}\r\n0\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut reply = Vec::new();
    let _ = stream.read_to_end(&mut reply).await;

    let summary = &w.service.captures(&l.config.id).unwrap()[0];
    assert!(summary.size >= 2048, "size {}", summary.size);
}
