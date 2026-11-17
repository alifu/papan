//! Turns the options read by [`super::import`] into a request.

use super::import::{file_not_read, Data, Parsed};
use crate::engine::http::types::CurlImport;
use crate::error::AppError;
use crate::store::types::{
    Auth, Body, KeyValue, RequestContent, Scripts, Settings, DEFAULT_TIMEOUT_MS,
};

/// What cURL sends for `-d`, `--data-raw`, `--data-binary` and `--data-urlencode`.
const FORM_TYPE: &str = "application/x-www-form-urlencoded";

/// Papan's own types for a text and an XML body (`engine/http/build.rs`).
const TEXT_TYPE: &str = "text/plain; charset=utf-8";
const XML_TYPE: &str = "application/xml";
const JSON_TYPE: &str = "application/json";

fn is_content_type(name: &str) -> bool {
    name.eq_ignore_ascii_case("content-type")
}

/// A `-d`-style body is sent by cURL as a form unless a Content-Type is given or removed.
/// A given type that Papan would send anyway is folded into the body type instead.
fn with_content_type(
    body: Body,
    headers: Vec<KeyValue>,
    removed: &[String],
) -> (Body, Vec<KeyValue>) {
    if !matches!(body, Body::Text { .. } | Body::Binary { .. }) {
        return (body, headers);
    }
    let given = headers
        .iter()
        .filter(|header| is_content_type(&header.name))
        .map(|header| header.value.as_str())
        .collect::<Vec<_>>();
    let folded = match (&body, given.as_slice()) {
        (Body::Text { .. }, [TEXT_TYPE]) => Some(body.clone()),
        (Body::Text { text }, [XML_TYPE]) => Some(Body::Xml { text: text.clone() }),
        (Body::Text { text }, [JSON_TYPE]) => Some(Body::Json { text: text.clone() }),
        _ => None,
    };
    if let Some(folded) = folded {
        let rest = headers
            .into_iter()
            .filter(|header| !is_content_type(&header.name))
            .collect();
        return (folded, rest);
    }
    if !given.is_empty() || removed.iter().any(|name| is_content_type(name)) {
        return (body, headers);
    }
    let form = KeyValue {
        name: "Content-Type".to_string(),
        value: FORM_TYPE.to_string(),
        enabled: true,
    };
    (body, headers.into_iter().chain([form]).collect())
}

/// The first URL; but after an unknown option, whose argument may be among the URLs,
/// the last one, named in a warning.
fn choose_url(parsed: &mut Parsed) -> String {
    let urls = std::mem::take(&mut parsed.urls);
    let several = urls.len() > 1;
    let chosen = if parsed.saw_unknown {
        urls.last()
    } else {
        urls.first()
    };
    let chosen = chosen.cloned().unwrap_or_default();
    if several && parsed.saw_unknown {
        parsed.warnings.push(format!(
            "several URLs after an unknown option; using the last one: {chosen}"
        ));
    } else if several {
        parsed
            .warnings
            .push("only the first URL is imported".to_string());
    }
    chosen
}

pub(super) fn build(mut parsed: Parsed) -> Result<CurlImport, AppError> {
    if parsed.urls.is_empty() {
        return Err(AppError::invalid_input("The cURL command has no URL."));
    }
    let mut url = choose_url(&mut parsed);
    let body = if parsed.get {
        url = with_query(&url, &data_texts(&mut parsed));
        Body::None
    } else {
        body_of(&mut parsed)
    };
    let method = parsed.method.clone().unwrap_or_else(|| {
        let implied = if parsed.head {
            "HEAD"
        } else if parsed.get || body == Body::None {
            "GET"
        } else {
            "POST"
        };
        implied.to_string()
    });
    let mut headers = std::mem::take(&mut parsed.headers);
    if !parsed.cookies.is_empty() {
        headers.push(KeyValue {
            name: "Cookie".to_string(),
            value: parsed.cookies.join("; "),
            enabled: true,
        });
    }
    let (body, headers) = with_content_type(body, headers, &parsed.removed_headers);
    let content = RequestContent {
        method,
        url,
        disabled_params: Vec::new(),
        headers,
        body,
        auth: auth_of(&parsed),
        settings: Settings {
            follow_redirects: parsed.follow_redirects,
            timeout_ms: parsed.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS),
            verify_tls: !parsed.insecure,
            http_version: parsed.http_version,
        },
        scripts: Scripts::default(),
        ..RequestContent::default()
    };
    Ok(CurlImport {
        content,
        warnings: parsed.warnings,
    })
}

/// The text data pieces; a file piece cannot be used here and is warned about.
fn data_texts(parsed: &mut Parsed) -> Vec<String> {
    let data = std::mem::take(&mut parsed.data);
    let mut texts = Vec::new();
    for piece in data {
        match piece {
            Data::Text(text) => texts.push(text),
            Data::File(_) => file_not_read(parsed, "-d"),
        }
    }
    texts
}

fn with_query(url: &str, pieces: &[String]) -> String {
    if pieces.is_empty() {
        return url.to_string();
    }
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}{}", pieces.join("&"))
}

fn body_of(parsed: &mut Parsed) -> Body {
    if !parsed.parts.is_empty() {
        if !parsed.data.is_empty() || !parsed.json.is_empty() {
            parsed
                .warnings
                .push("-d and --json are ignored because -F is used".to_string());
        }
        return Body::Multipart {
            parts: std::mem::take(&mut parsed.parts),
        };
    }
    if !parsed.json.is_empty() {
        if !parsed.data.is_empty() {
            parsed
                .warnings
                .push("-d is ignored because --json is used".to_string());
        }
        return Body::Json {
            text: parsed.json.concat(),
        };
    }
    if let [Data::File(path)] = parsed.data.as_slice() {
        return Body::Binary {
            file: Some(path.clone()),
        };
    }
    let texts = data_texts(parsed);
    if texts.is_empty() {
        Body::None
    } else {
        Body::Text {
            text: texts.join("&"),
        }
    }
}

fn auth_of(parsed: &Parsed) -> Auth {
    let Some(user) = &parsed.user else {
        return Auth::None;
    };
    let (username, password) = user.split_once(':').unwrap_or((user, ""));
    let (username, password) = (username.to_string(), password.to_string());
    if parsed.digest {
        Auth::Digest { username, password }
    } else {
        Auth::Basic { username, password }
    }
}
