//! Shared test values for the webhook unit tests.

use super::capture::build_capture;
use super::types::{Capture, WebhookBind, WebhookConfig, WebhookListener, WebhookResponse};

pub fn sample() -> WebhookConfig {
    WebhookConfig {
        id: "3f0c1c0e-6f55-4a52-8d6e-1d6f2d0b9c11".into(),
        name: "GitHub".into(),
        bind: WebhookBind::Loopback,
        port: 0,
        path: "/hooks".into(),
        response: WebhookResponse {
            status: 200,
            headers: vec![],
            body: String::new(),
        },
        max_body_bytes: super::config::DEFAULT_MAX_BODY,
        max_captures: super::config::DEFAULT_MAX_CAPTURES,
    }
}

pub fn sample_listener(port: Option<u16>) -> WebhookListener {
    WebhookListener {
        config: sample(),
        running: port.is_some(),
        url: port.map(|p| format!("http://127.0.0.1:{p}/hooks")),
        captured: 0,
        unmatched: 0,
    }
}

pub fn capture_of(
    method: &str,
    path: &str,
    query: &str,
    content_type: &str,
    text: Option<&str>,
) -> Capture {
    let uri = if query.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{query}")
    };
    let mut builder = http::Request::builder().method(method).uri(uri);
    if !content_type.is_empty() {
        builder = builder.header("Content-Type", content_type);
    }
    let parts = builder.body(()).map(|r| r.into_parts().0);
    let Ok(parts) = parts else {
        unreachable!("fixture request is valid")
    };
    build_capture(
        &parts,
        "127.0.0.1:1",
        Some(text.unwrap_or("").as_bytes()),
        0,
    )
}
