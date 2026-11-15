//! Turns a capture into a scratch request (plan 07 §6, D7). Pure: the caller saves it.

use super::types::{Capture, WebhookListener};
use crate::store::types::{Body, KeyValue, RequestContent};

/// Hop-by-hop and framing headers that belong to the original connection.
const DROPPED_HEADERS: [&str; 8] = [
    "host",
    "content-length",
    "connection",
    "transfer-encoding",
    "keep-alive",
    "upgrade",
    "te",
    "trailer",
];

pub const SENSITIVE_HEADERS_WARNING: &str =
    "Authorization/Cookie/signature headers were copied; remove them before saving to a collection.";
pub const NO_HOST_WARNING: &str =
    "The listener is stopped and its port is chosen on start, so the URL has no host. Start the listener, or add the host and port.";

/// Stops captured text from being read as one of the user's variables: `{{` becomes the
/// resolver's escape `\{{`.
fn literal(text: &str) -> String {
    text.replace("{{", "\\{{")
}

fn is_sensitive(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "authorization",
        "cookie",
        "signature",
        "token",
        "secret",
        "api-key",
    ]
    .iter()
    .any(|part| name.contains(part))
}

pub fn capture_to_content(
    capture: &Capture,
    listener: &WebhookListener,
) -> (RequestContent, Vec<String>) {
    let mut warnings = Vec::new();
    let headers = capture
        .headers
        .iter()
        .filter(|h| !DROPPED_HEADERS.contains(&h.name.to_ascii_lowercase().as_str()))
        .map(|h| KeyValue {
            name: literal(&h.name),
            value: literal(&h.value),
            enabled: h.enabled,
        })
        .collect::<Vec<_>>();
    if headers.iter().any(|h| is_sensitive(&h.name)) {
        warnings.push(SENSITIVE_HEADERS_WARNING.into());
    }
    let body = body_for(capture, &mut warnings);
    let url = replay_url(capture, listener, &mut warnings);
    let content = RequestContent {
        headers,
        body,
        ..RequestContent::new(&capture.summary.method, url)
    };
    (content, warnings)
}

/// The scratch request's name, for example `POST /hooks 12:04:05` (UTC).
pub fn replay_name(capture: &Capture) -> String {
    let seconds = (capture.summary.at_ms / 1000.0) as i64;
    let clock = time::OffsetDateTime::from_unix_timestamp(seconds)
        .map(|at| format!("{:02}:{:02}:{:02}", at.hour(), at.minute(), at.second()))
        .unwrap_or_default();
    format!(
        "{} {} {clock}",
        capture.summary.method, capture.summary.path
    )
    .trim()
    .to_string()
}

/// The listener's origin (an any-interface address becomes loopback) + path + query.
/// A stopped listener on port 0 has no port yet: the URL then has no host.
fn replay_url(capture: &Capture, listener: &WebhookListener, warnings: &mut Vec<String>) -> String {
    let running_port = listener
        .url
        .as_deref()
        .and_then(|url| url::Url::parse(url).ok())
        .and_then(|url| url.port());
    let port =
        running_port.or_else(|| u16::try_from(listener.config.port).ok().filter(|p| *p != 0));
    let origin = match port {
        Some(port) => format!("http://127.0.0.1:{port}"),
        None => {
            warnings.push(NO_HOST_WARNING.into());
            String::new()
        }
    };
    let mut url = format!("{origin}{}", literal(&capture.summary.path));
    if !capture.query.is_empty() {
        url.push('?');
        url.push_str(&literal(&capture.query));
    }
    url
}

fn essence(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

fn body_for(capture: &Capture, warnings: &mut Vec<String>) -> Body {
    if capture.truncated {
        warnings.push("The body was over the size limit and was not captured.".into());
        return Body::None;
    }
    let Some(text) = capture.body_text.as_deref().map(literal) else {
        if capture.body_base64.is_some() {
            warnings.push("The body is binary and was not copied.".into());
        }
        return Body::None;
    };
    let kind = essence(&capture.summary.content_type);
    if kind == "application/x-www-form-urlencoded" {
        let fields = url::form_urlencoded::parse(
            capture.body_text.as_deref().unwrap_or_default().as_bytes(),
        )
        .map(|(name, value)| KeyValue {
            name: literal(&name),
            value: literal(&value),
            enabled: true,
        })
        .collect();
        return Body::Form { fields };
    }
    if kind == "application/json" || kind.ends_with("+json") {
        Body::Json { text }
    } else if kind == "application/xml" || kind == "text/xml" || kind.ends_with("+xml") {
        Body::Xml { text }
    } else {
        Body::Text { text }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::webhook::fixtures::{capture_of, sample_listener};

    fn replay(content_type: &str, text: Option<&str>) -> (RequestContent, Vec<String>) {
        let mut capture = capture_of("POST", "/hooks/x", "a=1", content_type, text);
        capture.headers.push(KeyValue {
            name: "Host".into(),
            value: "h".into(),
            enabled: true,
        });
        capture_to_content(&capture, &sample_listener(Some(4321)))
    }

    #[test]
    fn json_xml_text_and_form_bodies_map_by_content_type() {
        let cases = [
            ("application/json; charset=utf-8", "{}", "json"),
            ("application/vnd.api+json", "{}", "json"),
            ("text/xml", "<a/>", "xml"),
            ("application/soap+xml", "<a/>", "xml"),
            ("text/plain", "hi", "text"),
            ("", "hi", "text"),
        ];
        for (content_type, text, want) in cases {
            let (content, warnings) = replay(content_type, Some(text));
            let got = match content.body {
                Body::Json { .. } => "json",
                Body::Xml { .. } => "xml",
                Body::Text { .. } => "text",
                _ => "other",
            };
            assert_eq!(got, want, "{content_type}");
            assert!(warnings.is_empty());
        }
        let (content, _) = replay("application/x-www-form-urlencoded", Some("a=1&b=x%20y"));
        let Body::Form { fields } = content.body else {
            panic!("expected a form body");
        };
        assert_eq!(
            (fields[1].name.as_str(), fields[1].value.as_str()),
            ("b", "x y")
        );
    }

    #[test]
    fn url_uses_listener_port_path_and_query() {
        let (content, _) = replay("text/plain", Some("x"));
        assert_eq!(content.method, "POST");
        assert_eq!(content.url, "http://127.0.0.1:4321/hooks/x?a=1");
    }

    #[test]
    fn framing_headers_are_dropped_and_others_kept() {
        let (content, _) = replay("text/plain", Some("x"));
        let names: Vec<_> = content
            .headers
            .iter()
            .map(|h| h.name.to_lowercase())
            .collect();
        assert!(!names.contains(&"host".to_string()));
        assert!(names.contains(&"content-type".to_string()));
    }

    #[test]
    fn binary_and_truncated_bodies_warn() {
        let mut binary = capture_of("POST", "/h", "", "application/octet-stream", None);
        binary.body_base64 = Some("/wD+".into());
        let (content, warnings) = capture_to_content(&binary, &sample_listener(Some(1)));
        assert_eq!(content.body, Body::None);
        assert_eq!(warnings.len(), 1);
        let mut cut = capture_of("POST", "/h", "", "text/plain", None);
        cut.truncated = true;
        assert_eq!(
            capture_to_content(&cut, &sample_listener(Some(1))).1.len(),
            1
        );
    }

    #[test]
    fn empty_body_is_none_without_warning() {
        let (content, warnings) = replay("text/plain", None);
        assert_eq!(content.body, Body::None);
        assert!(warnings.is_empty());
    }

    #[test]
    fn name_has_method_path_and_time() {
        let mut capture = capture_of("POST", "/hooks", "", "", None);
        capture.summary.at_ms = 43_445_000.0; // 12:04:05 UTC on 1970-01-01
        assert_eq!(replay_name(&capture), "POST /hooks 12:04:05");
    }

    #[test]
    fn braces_in_captured_data_are_escaped_so_variables_stay_out_of_reach() {
        let mut capture = capture_of(
            "POST",
            "/h/{{id}}",
            "q={{secret}}",
            "text/plain",
            Some("{{token}}"),
        );
        capture.headers.push(KeyValue {
            name: "X-A".into(),
            value: "{{pw}}".into(),
            enabled: true,
        });
        let (content, _) = capture_to_content(&capture, &sample_listener(Some(1)));

        assert_eq!(content.url, "http://127.0.0.1:1/h/\\{{id}}?q=\\{{secret}}");
        assert!(matches!(content.body, Body::Text { ref text } if text == "\\{{token}}"));
        assert!(content.headers.iter().any(|h| h.value == "\\{{pw}}"));
    }

    #[test]
    fn sensitive_headers_are_copied_with_a_warning() {
        let mut capture = capture_of("POST", "/h", "", "text/plain", Some("x"));
        capture.headers.push(KeyValue {
            name: "X-Hub-Signature-256".into(),
            value: "sha256=1".into(),
            enabled: true,
        });

        let (content, warnings) = capture_to_content(&capture, &sample_listener(Some(1)));

        assert!(content
            .headers
            .iter()
            .any(|h| h.name == "X-Hub-Signature-256"));
        assert_eq!(warnings, [SENSITIVE_HEADERS_WARNING]);
    }

    #[test]
    fn a_stopped_port_zero_listener_gives_a_hostless_url_and_a_warning() {
        let capture = capture_of("POST", "/hooks/x", "a=1", "text/plain", Some("x"));

        let (content, warnings) = capture_to_content(&capture, &sample_listener(None));

        assert_eq!(content.url, "/hooks/x?a=1");
        assert_eq!(warnings, [NO_HOST_WARNING]);
    }
}
