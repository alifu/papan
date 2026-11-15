//! The configured reply (plan 07 §6). No CORS headers unless the user adds them, so a
//! web page that posts to the listener cannot read the answer. No templating.

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::Response;

use super::types::WebhookResponse;

pub type Reply = Response<Full<Bytes>>;

const DEFAULT_CONTENT_TYPE: &str = "text/plain; charset=utf-8";

fn reply(status: u16, content_type: &'static str, body: Bytes) -> Reply {
    let mut response = Response::new(Full::new(body));
    *response.status_mut() = http::StatusCode::from_u16(status).unwrap_or(http::StatusCode::OK);
    response.headers_mut().insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static(content_type),
    );
    response
}

/// A fixed plain-text error such as `404` or `413`.
pub fn plain(status: u16, text: &'static str) -> Reply {
    reply(
        status,
        DEFAULT_CONTENT_TYPE,
        Bytes::from_static(text.as_bytes()),
    )
}

/// The listener's response; invalid headers (config validation refuses them) are skipped.
pub fn configured(config: &WebhookResponse) -> Reply {
    let mut response = reply(
        u16::try_from(config.status).unwrap_or(200),
        DEFAULT_CONTENT_TYPE,
        Bytes::from(config.body.clone()),
    );
    for header in config.headers.iter().filter(|h| h.enabled) {
        let pair = (
            http::HeaderName::from_bytes(header.name.as_bytes()),
            http::HeaderValue::from_str(&header.value),
        );
        if let (Ok(name), Ok(value)) = pair {
            if name == http::header::CONTENT_TYPE {
                response.headers_mut().insert(name, value);
            } else {
                response.headers_mut().append(name, value);
            }
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::types::KeyValue;

    fn header(name: &str, value: &str, enabled: bool) -> KeyValue {
        KeyValue {
            name: name.into(),
            value: value.into(),
            enabled,
        }
    }

    #[test]
    fn default_reply_is_empty_text_200() {
        let response = configured(&WebhookResponse {
            status: 200,
            headers: vec![],
            body: String::new(),
        });
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], DEFAULT_CONTENT_TYPE);
        assert!(!response
            .headers()
            .contains_key("access-control-allow-origin"));
    }

    #[test]
    fn configured_headers_replace_default_content_type() {
        let response = configured(&WebhookResponse {
            status: 201,
            headers: vec![
                header("Content-Type", "application/json", true),
                header("X-Skip", "1", false),
            ],
            body: "{}".into(),
        });
        assert_eq!(response.status(), 201);
        assert_eq!(response.headers().get_all("content-type").iter().count(), 1);
        assert_eq!(response.headers()["content-type"], "application/json");
        assert!(!response.headers().contains_key("x-skip"));
    }
}
