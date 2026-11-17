//! Cancel, body cap and previews (plan 03 §10, `http_stream`).

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use papan_lib::engine::http::responses::{PREVIEW_TEXT_BYTES, RESPONSES_DIR};
use papan_lib::engine::http::types::BodyEncoding;
use papan_lib::engine::http::HttpEngine;
use papan_lib::error::ErrorCode;
use papan_lib::store::types::{RequestContent, RequestLocation};
use support::server::{self, fixture_dir};

const CANCEL_LIMIT: Duration = Duration::from_secs(1);

fn engine(dir: &tempfile::TempDir) -> HttpEngine {
    HttpEngine::new(&dir.path().join("cache"), dir.path(), "1").unwrap()
}

fn scratch() -> RequestLocation {
    RequestLocation::Scratch { id: "s".into() }
}

fn response_files(dir: &tempfile::TempDir) -> usize {
    std::fs::read_dir(dir.path().join("cache").join(RESPONSES_DIR))
        .unwrap()
        .count()
}

async fn cancel_after(path: &str, wait: Duration) {
    let server = server::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Arc::new(engine(&dir));
    let id = uuid::Uuid::new_v4().to_string();
    let sending = Arc::clone(&engine);
    let (task_id, url) = (id.clone(), server.url(path));
    let task = tokio::spawn(async move {
        sending
            .send(&task_id, &scratch(), &RequestContent::new("GET", url))
            .await
    });
    tokio::time::sleep(wait).await;
    let second = engine
        .send(
            &id,
            &scratch(),
            &RequestContent::new("GET", server.url("/status/200")),
        )
        .await;

    let started = Instant::now();
    engine.cancel(&id).unwrap();
    let result = task.await.unwrap();

    assert!(started.elapsed() < CANCEL_LIMIT);
    assert_eq!(second.unwrap_err().code, ErrorCode::InvalidInput);
    assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
    assert_eq!(engine.in_flight(), 0);
    assert_eq!(response_files(&dir), 0, "{path} left a file");
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_while_waiting_for_headers() {
    cancel_after("/delay/5000", Duration::from_millis(200)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_mid_body() {
    cancel_after("/drip", Duration::from_millis(350)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_body_over_the_cap_is_truncated_at_the_cap() {
    let server = server::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(&dir).with_max_body_bytes(1000);
    let id = uuid::Uuid::new_v4().to_string();

    let meta = engine
        .send(
            &id,
            &scratch(),
            &RequestContent::new("GET", server.url("/bytes/5000")),
        )
        .await
        .unwrap();

    assert!(meta.truncated);
    assert_eq!(meta.size_bytes, 1000);
    let file = engine.body_file(&id).unwrap();
    assert_eq!(std::fs::metadata(file).unwrap().len(), 1000);
}

async fn preview(path: &str) -> papan_lib::engine::http::types::ResponseBody {
    let server = server::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(&dir);
    let id = uuid::Uuid::new_v4().to_string();
    engine
        .send(
            &id,
            &scratch(),
            &RequestContent::new("GET", server.url(path)),
        )
        .await
        .unwrap();
    engine.read_body(&id).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn previews_decode_text_images_and_skip_binaries() {
    let long = preview(&format!("/bytes/{}", PREVIEW_TEXT_BYTES + 10)).await;
    let latin1 = preview("/fixture/latin1.txt").await;
    let png = preview("/fixture/pixel.png").await;
    let binary = preview("/fixture/upload.bin").await;
    let pixel = std::fs::read(fixture_dir().join("pixel.png")).unwrap();

    assert!(long.truncated);
    assert_eq!(long.data.len(), PREVIEW_TEXT_BYTES);
    assert!(latin1.data.starts_with("Café naïve"), "{}", latin1.data);
    assert_eq!(png.encoding, BodyEncoding::Base64);
    assert_eq!(
        png.data,
        base64::engine::general_purpose::STANDARD.encode(pixel)
    );
    assert_eq!(
        (binary.encoding, binary.data.as_str()),
        (BodyEncoding::None, "")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn ids_that_are_not_uuids_are_rejected_by_every_entry_point() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(&dir);
    let absolute = dir.path().join("x").display().to_string();

    for id in ["../x", absolute.as_str(), ""] {
        let content = RequestContent::new("GET", "http://127.0.0.1:9/");
        let errors = [
            engine.send(id, &scratch(), &content).await.unwrap_err(),
            engine.cancel(id).unwrap_err(),
            engine.read_body(id).unwrap_err(),
            engine.body_file(id).unwrap_err(),
        ];
        for error in errors {
            assert_eq!(error.code, ErrorCode::InvalidInput, "{id:?}");
        }
    }
}
