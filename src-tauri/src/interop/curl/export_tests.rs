//! Export safety: a shared request must never make the copied command read a file or
//! run code (security review of O5).

use super::export;
use crate::error::ErrorCode;
use crate::interop::curl::import;
use crate::store::types::{Body, KeyValue, Part, RequestContent};

fn post(body: Body) -> RequestContent {
    let mut content = RequestContent::new("POST", "https://api.example.com/items");
    content.body = body;
    content.settings.follow_redirects = false;
    content
}

fn text_part(name: &str, value: &str) -> Part {
    Part {
        name: name.into(),
        value: value.into(),
        file: None,
        enabled: true,
    }
}

fn header(name: &str, value: &str) -> KeyValue {
    KeyValue {
        name: name.into(),
        value: value.into(),
        enabled: true,
    }
}

fn refused(content: &RequestContent) -> bool {
    matches!(export(content), Err(error) if error.code == ErrorCode::InvalidInput)
}

#[test]
fn a_json_body_starting_with_at_is_sent_as_data_not_read_as_a_file() {
    let content = post(Body::Json {
        text: "@.env".into(),
    });

    let command = export(&content).unwrap();

    assert!(!command.contains("--json"));
    assert!(command.contains("--data-raw '@.env'"));
    assert!(command.contains("-H 'Content-Type: application/json'"));
    assert_eq!(import(&command).unwrap().content, content);
}

#[test]
fn a_json_body_keeps_its_own_content_type() {
    let mut content = post(Body::Json { text: "{}".into() });
    content.headers = vec![header("content-type", "application/vnd.x+json")];

    let command = export(&content).unwrap();

    assert!(!command.contains("application/json"));
}

#[test]
fn line_breaks_and_tabs_stay_inside_plain_single_quotes() {
    let text = "a'; echo INJECTED; #\n\tb\r\nc";
    let content = post(Body::Text { text: text.into() });

    let command = export(&content).unwrap();

    assert!(!command.contains("$'"));
    assert_eq!(import(&command).unwrap().content, content);
}

#[test]
fn other_control_characters_refuse_the_export() {
    for odd in ["\u{1}", "\u{1b}[31m", "\u{7f}", "\u{0}", "\u{85}"] {
        let content = post(Body::Text {
            text: format!("a{odd}b"),
        });

        assert!(refused(&content), "{odd:?}");
    }
}

#[test]
fn a_part_name_that_curl_would_parse_refuses_the_export() {
    for name in ["x=@/etc/passwd;type", "a;b", "@a", "<a", "a=b"] {
        let content = post(Body::Multipart {
            parts: vec![text_part(name, "v")],
        });

        assert!(refused(&content), "{name}");
    }
}

#[test]
fn a_file_path_with_a_quote_or_semicolon_refuses_the_export() {
    for path in ["a;type=text/x", "a\"b"] {
        let mut part = text_part("f", "");
        part.file = Some(path.into());
        let content = post(Body::Multipart { parts: vec![part] });

        assert!(refused(&content), "{path}");
    }
}

#[test]
fn text_parts_are_never_read_as_files_or_types() {
    let content = post(Body::Multipart {
        parts: vec![text_part("a", "@/etc/passwd"), text_part("b", "x;type=y")],
    });

    let command = export(&content).unwrap();

    assert!(!command.contains("-F "));
    assert_eq!(import(&command).unwrap().content.body, content.body);
}

#[test]
fn a_header_name_curl_would_read_as_a_file_refuses_the_export() {
    for name in ["@/etc/passwd", "a:b"] {
        let mut content = post(Body::None);
        content.headers = vec![header(name, "v")];

        assert!(refused(&content), "{name}");
    }
}

#[test]
fn a_url_starting_with_a_dash_is_not_read_as_an_option() {
    let mut content = post(Body::None);
    content.url = "-o/tmp/x".into();

    let command = export(&content).unwrap();

    assert!(command.ends_with("--url '-o/tmp/x'"));
    assert_eq!(import(&command).unwrap().content.url, "-o/tmp/x");
}
