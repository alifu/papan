//! Postman request bodies to our `Body` (plan 09 §3 "Request mapping"). File paths found
//! in the file are kept as text only; they are never read.

use crate::interop::postman::schema;
use crate::store::types::{Body, KeyValue, Part};

/// Our body, and report messages about what was lost on the way.
pub fn map_body(body: Option<&schema::Body>, content_type: Option<&str>) -> (Body, Vec<String>) {
    let Some(body) = body else {
        return (Body::None, Vec::new());
    };
    if body.disabled {
        return (
            Body::None,
            vec!["The body was switched off in Postman and was not imported.".into()],
        );
    }
    match body.mode.as_deref() {
        None | Some("none") => (Body::None, Vec::new()),
        Some("raw") => (raw(body, content_type), Vec::new()),
        Some("urlencoded") => (form(&body.urlencoded), Vec::new()),
        Some("formdata") => multipart(&body.formdata),
        Some("file") => binary(body.file.as_ref()),
        Some("graphql") => (graphql(body.graphql.as_ref()), Vec::new()),
        Some(_) => (
            Body::None,
            vec!["The body mode is not supported; the request has no body.".into()],
        ),
    }
}

fn raw(body: &schema::Body, content_type: Option<&str>) -> Body {
    let text = body.raw.clone().unwrap_or_default();
    if text.is_empty() {
        return Body::None;
    }
    let language = body
        .options
        .as_ref()
        .map(|options| options.raw.language.to_ascii_lowercase())
        .filter(|language| !language.is_empty());
    let language = language.unwrap_or_else(|| from_content_type(content_type).to_string());
    match language.as_str() {
        "json" => Body::Json { text },
        "xml" => Body::Xml { text },
        _ => Body::Text { text },
    }
}

fn from_content_type(content_type: Option<&str>) -> &'static str {
    let lower = content_type.unwrap_or_default().to_ascii_lowercase();
    if lower.contains("json") {
        "json"
    } else if lower.contains("xml") {
        "xml"
    } else {
        "text"
    }
}

fn form(params: &[schema::FormParam]) -> Body {
    let fields = params
        .iter()
        .map(|param| KeyValue {
            name: param.key.clone(),
            value: param.value.clone().unwrap_or_default(),
            enabled: !param.disabled,
        })
        .collect();
    Body::Form { fields }
}

fn multipart(params: &[schema::FormParam]) -> (Body, Vec<String>) {
    let mut notes = Vec::new();
    let parts = params
        .iter()
        .map(|param| {
            let is_file = param.kind.as_deref() == Some("file");
            if is_file && param.src.len() > 1 {
                notes.push(format!(
                    "Part `{}` had several files; only the first was kept.",
                    param.key
                ));
            }
            if param.content_type.is_some() {
                notes.push(format!(
                    "The content type of part `{}` was dropped.",
                    param.key
                ));
            }
            // The path is the exporter's machine path: never kept, never read (plan 09 §3).
            if is_file {
                let path = param.src.first().map_or("", String::as_str);
                notes.push(format!(
                    "Re-pick the file of part `{}` (it was `{path}`).",
                    param.key
                ));
            }
            Part {
                name: param.key.clone(),
                value: if is_file {
                    String::new()
                } else {
                    param.value.clone().unwrap_or_default()
                },
                file: None,
                enabled: !param.disabled,
            }
        })
        .collect();
    (Body::Multipart { parts }, notes)
}

fn binary(file: Option<&schema::FileBody>) -> (Body, Vec<String>) {
    let src = file
        .and_then(|file| file.src.clone())
        .filter(|src| !src.is_empty());
    let notes = src
        .map(|path| format!("Re-pick the body file (it was `{path}`): paths are never imported."));
    (Body::Binary { file: None }, notes.into_iter().collect())
}

fn graphql(body: Option<&schema::GraphqlBody>) -> Body {
    let body = body.cloned().unwrap_or_default();
    Body::Graphql {
        query: body.query,
        variables: body.variables,
        operation_name: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interop::postman::schema::{BodyOptions, FileBody, FormParam, RawOptions};

    fn raw_body(text: &str, language: &str) -> schema::Body {
        schema::Body {
            mode: Some("raw".into()),
            raw: Some(text.into()),
            options: (!language.is_empty()).then(|| BodyOptions {
                raw: RawOptions {
                    language: language.into(),
                },
            }),
            ..schema::Body::default()
        }
    }

    #[test]
    fn raw_language_then_content_type_picks_the_body_type() {
        let cases = [
            (
                raw_body("{}", "json"),
                None,
                Body::Json { text: "{}".into() },
            ),
            (
                raw_body("<a/>", "xml"),
                None,
                Body::Xml {
                    text: "<a/>".into(),
                },
            ),
            (
                raw_body("x", "javascript"),
                None,
                Body::Text { text: "x".into() },
            ),
            (raw_body("x", "html"), None, Body::Text { text: "x".into() }),
            (
                raw_body("{}", ""),
                Some("application/vnd.api+json"),
                Body::Json { text: "{}".into() },
            ),
            (
                raw_body("<a/>", ""),
                Some("text/xml"),
                Body::Xml {
                    text: "<a/>".into(),
                },
            ),
            (raw_body("x", ""), None, Body::Text { text: "x".into() }),
            (raw_body("", "json"), None, Body::None),
        ];
        for (body, content_type, expected) in cases {
            assert_eq!(map_body(Some(&body), content_type).0, expected);
        }
    }

    #[test]
    fn forms_keep_disabled_rows_switched_off() {
        let body = schema::Body {
            mode: Some("urlencoded".into()),
            urlencoded: vec![FormParam {
                key: "a".into(),
                value: Some("1".into()),
                disabled: true,
                ..FormParam::default()
            }],
            ..schema::Body::default()
        };

        assert_eq!(
            map_body(Some(&body), None).0,
            Body::Form {
                fields: vec![KeyValue {
                    name: "a".into(),
                    value: "1".into(),
                    enabled: false
                }]
            }
        );
    }

    #[test]
    fn multipart_keeps_the_first_file_and_reports_losses() {
        let body = schema::Body {
            mode: Some("formdata".into()),
            formdata: vec![
                FormParam {
                    key: "f".into(),
                    src: vec!["/a/x.png".into(), "/a/y.png".into()],
                    kind: Some("file".into()),
                    content_type: Some("image/png".into()),
                    ..FormParam::default()
                },
                FormParam {
                    key: "t".into(),
                    value: Some("v".into()),
                    kind: Some("text".into()),
                    ..FormParam::default()
                },
            ],
            ..schema::Body::default()
        };

        let (mapped, notes) = map_body(Some(&body), None);

        let Body::Multipart { parts } = mapped else {
            panic!("multipart expected");
        };
        assert_eq!(parts[0].file, None, "paths are never imported");
        assert!(notes.iter().any(|n| n.contains("/a/x.png")));
        assert_eq!(parts[1].value, "v");
        assert_eq!(parts[1].file, None);
        assert_eq!(notes.len(), 3, "{notes:?}");
    }

    #[test]
    fn file_graphql_disabled_and_unknown_modes() {
        let file = schema::Body {
            mode: Some("file".into()),
            file: Some(FileBody {
                src: Some("C:\\x.bin".into()),
            }),
            ..schema::Body::default()
        };
        let graphql = schema::Body {
            mode: Some("graphql".into()),
            graphql: Some(schema::GraphqlBody {
                query: "{a}".into(),
                variables: "{}".into(),
            }),
            ..schema::Body::default()
        };
        let disabled = schema::Body {
            disabled: true,
            ..raw_body("x", "")
        };
        let unknown = schema::Body {
            mode: Some("weird".into()),
            ..schema::Body::default()
        };

        let (binary, notes) = map_body(Some(&file), None);
        assert_eq!(
            binary,
            Body::Binary { file: None },
            "paths are never imported"
        );
        assert!(notes.iter().any(|n| n.contains("C:\\x.bin")));
        assert_eq!(notes.len(), 1);
        assert_eq!(
            map_body(Some(&graphql), None).0,
            Body::Graphql {
                query: "{a}".into(),
                variables: "{}".into(),
                operation_name: String::new()
            }
        );
        assert_eq!(map_body(Some(&disabled), None).0, Body::None);
        assert_eq!(map_body(Some(&unknown), None).1.len(), 1);
        assert_eq!(map_body(None, None), (Body::None, Vec::new()));
    }
}
