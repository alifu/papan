//! Request metadata from the headers table and the auth section (plan 08 §5), and the
//! masked form of metadata that events show. gRPC has no cookies and no query string.

use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use base64::Engine as _;
use http::header::{HeaderMap, HeaderName, HeaderValue};

use crate::engine::http::build::auth_headers;
use crate::engine::ws::messages::Masker;
use crate::error::AppError;
use crate::logging::redact::is_sensitive_header;
use crate::store::types::{ApiKeyPlacement, Auth, KeyValue, RequestContent};

pub const MAX_ENTRIES: usize = 100;
const BIN_SUFFIX: &str = "-bin";
const RESERVED: [&str; 9] = [
    "content-type",
    "te",
    "host",
    "connection",
    "transfer-encoding",
    "upgrade",
    "keep-alive",
    "proxy-connection",
    "content-length",
];
const MASK: &str = "***";

fn check_name(name: &str) -> Result<HeaderName, AppError> {
    let lower = name.trim().to_ascii_lowercase();
    let reserved =
        lower.starts_with(':') || lower.starts_with("grpc-") || RESERVED.contains(&lower.as_str());
    if reserved {
        return Err(AppError::invalid_input(format!(
            "The metadata name `{name}` is reserved by gRPC."
        )));
    }
    HeaderName::from_bytes(lower.as_bytes())
        .map_err(|_| AppError::invalid_input(format!("The metadata name `{name}` is not valid.")))
}

/// `-bin` values are typed as base64 (padded or not) and sent unpadded.
fn value_of(name: &HeaderName, value: &str) -> Result<HeaderValue, AppError> {
    let unsendable = || {
        AppError::invalid_input(format!(
            "The value of `{name}` contains characters that cannot be sent."
        ))
    };
    if !name.as_str().ends_with(BIN_SUFFIX) {
        return HeaderValue::from_str(value).map_err(|_| unsendable());
    }
    let compact: String = value.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = STANDARD
        .decode(&compact)
        .or_else(|_| STANDARD_NO_PAD.decode(&compact))
        .map_err(|_| {
            AppError::invalid_input(format!("The value of `{name}` must be base64 (`-bin`)."))
        })?;
    HeaderValue::from_str(&STANDARD_NO_PAD.encode(bytes)).map_err(|_| unsendable())
}

fn refuse_unsupported(auth: &Auth) -> Result<(), AppError> {
    match auth {
        Auth::Digest { .. } => Err(AppError::invalid_input("Digest is not supported for gRPC.")),
        Auth::ApiKey {
            placement: ApiKeyPlacement::Query,
            ..
        } => Err(AppError::invalid_input(
            "gRPC has no query string; send the API key as metadata.",
        )),
        _ => Ok(()),
    }
}

/// The metadata of a resolved request: the enabled rows, then what the auth section adds
/// unless a row already set that name. OAuth 2 has already become Bearer.
pub fn build(content: &RequestContent) -> Result<HeaderMap, AppError> {
    refuse_unsupported(&content.auth)?;
    let rows: Vec<&KeyValue> = content
        .headers
        .iter()
        .filter(|row| row.enabled && !row.name.trim().is_empty())
        .collect();
    if rows.len() > MAX_ENTRIES {
        return Err(AppError::invalid_input(format!(
            "At most {MAX_ENTRIES} metadata entries can be sent."
        )));
    }
    let mut metadata = HeaderMap::new();
    for row in rows {
        let name = check_name(&row.name)?;
        let value = value_of(&name, &row.value)?;
        metadata.append(name, value);
    }
    for (name, value) in auth_headers(&content.auth)? {
        let name = check_name(name.as_str())?;
        if !metadata.contains_key(&name) {
            metadata.insert(name, value);
        }
    }
    Ok(metadata)
}

fn is_secret_name(name: &str) -> bool {
    is_sensitive_header(name) || name.ends_with("-token") || name.ends_with("-token-bin")
}

/// Metadata as events show it: values of secret-bearing names hidden, secrets masked.
pub fn shown(metadata: &HeaderMap, masker: &Masker) -> Vec<KeyValue> {
    metadata
        .iter()
        .map(|(name, value)| {
            let text = String::from_utf8_lossy(value.as_bytes());
            let value = if is_secret_name(name.as_str()) {
                MASK.to_string()
            } else {
                masker.mask(&text)
            };
            KeyValue {
                name: name.as_str().to_string(),
                value,
                enabled: true,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, value: &str) -> KeyValue {
        KeyValue {
            name: name.into(),
            value: value.into(),
            enabled: true,
        }
    }

    fn content(headers: Vec<KeyValue>, auth: Auth) -> RequestContent {
        RequestContent {
            headers,
            auth,
            ..RequestContent::default()
        }
    }

    fn get<'a>(map: &'a HeaderMap, name: &str) -> Vec<&'a str> {
        map.get_all(name)
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect()
    }

    #[test]
    fn rows_become_metadata_and_disabled_or_unnamed_rows_are_skipped() {
        let off = KeyValue {
            enabled: false,
            ..row("x-off", "1")
        };
        let rows = vec![
            row("X-Tenant", "a"),
            row("x-tenant", "b"),
            off,
            row(" ", "x"),
        ];

        let metadata = build(&content(rows, Auth::None)).unwrap();

        assert_eq!(get(&metadata, "x-tenant"), ["a", "b"]);
        assert_eq!(metadata.len(), 2);
    }

    #[test]
    fn bin_values_are_base64_and_sent_unpadded() {
        let rows = vec![row("trace-bin", "AAE="), row("other-bin", "AAEC")];

        let metadata = build(&content(rows, Auth::None)).unwrap();

        assert_eq!(get(&metadata, "trace-bin"), ["AAE"]);
        assert_eq!(get(&metadata, "other-bin"), ["AAEC"]);
        let bad = build(&content(vec![row("trace-bin", "not base64!")], Auth::None));
        assert!(bad.unwrap_err().message.contains("must be base64"));
    }

    #[test]
    fn auth_adds_metadata_unless_a_row_set_the_name() {
        let bearer = Auth::Bearer { token: "t1".into() };
        let basic = Auth::Basic {
            username: "u".into(),
            password: "p".into(),
        };
        let api_key = Auth::ApiKey {
            name: "X-Key".into(),
            value: "k".into(),
            placement: ApiKeyPlacement::Header,
        };

        let with_bearer = build(&content(Vec::new(), bearer.clone())).unwrap();
        let with_basic = build(&content(Vec::new(), basic)).unwrap();
        let with_key = build(&content(Vec::new(), api_key)).unwrap();
        let user_wins = build(&content(vec![row("authorization", "mine")], bearer)).unwrap();

        assert_eq!(get(&with_bearer, "authorization"), ["Bearer t1"]);
        assert_eq!(get(&with_basic, "authorization"), ["Basic dTpw"]);
        assert_eq!(get(&with_key, "x-key"), ["k"]);
        assert_eq!(get(&user_wins, "authorization"), ["mine"]);
    }

    #[test]
    fn digest_query_keys_reserved_names_and_bad_values_are_refused() {
        let digest = Auth::Digest {
            username: "u".into(),
            password: "p".into(),
        };
        let query = Auth::ApiKey {
            name: "key".into(),
            value: "k".into(),
            placement: ApiKeyPlacement::Query,
        };
        let message = |headers, auth| build(&content(headers, auth)).unwrap_err().message;

        assert_eq!(
            message(Vec::new(), digest),
            "Digest is not supported for gRPC."
        );
        assert!(message(Vec::new(), query).contains("no query string"));
        for name in [
            ":path",
            "grpc-timeout",
            "Content-Type",
            "te",
            "host",
            "connection",
            "Transfer-Encoding",
            "upgrade",
            "keep-alive",
            "proxy-connection",
            "content-length",
        ] {
            assert!(
                message(vec![row(name, "x")], Auth::None).contains("reserved"),
                "{name}"
            );
        }
        assert!(message(vec![row("bad name", "x")], Auth::None).contains("not valid"));
        assert!(message(vec![row("x-a", "line\nbreak")], Auth::None).contains("cannot be sent"));
        let many = (0..=MAX_ENTRIES)
            .map(|n| row(&format!("x-{n}"), "v"))
            .collect();
        assert!(message(many, Auth::None).contains("At most 100"));
    }

    #[test]
    fn shown_metadata_hides_secret_names_and_masks_secret_values() {
        let mut metadata = HeaderMap::new();
        for (name, value) in [
            ("authorization", "Bearer abc"),
            ("set-cookie", "sid=1"),
            ("x-session-token", "tok"),
            ("x-note", "hello s3cret"),
        ] {
            metadata.insert(
                HeaderName::from_static(name),
                HeaderValue::from_static(value),
            );
        }
        let masker = Masker::new(vec!["s3cret".into()]);

        let rows = shown(&metadata, &masker);

        let value = |name: &str| {
            let found = rows.iter().find(|row| row.name == name).unwrap();
            found.value.clone()
        };
        assert_eq!(value("authorization"), "***");
        assert_eq!(value("set-cookie"), "***");
        assert_eq!(value("x-session-token"), "***");
        assert!(!value("x-note").contains("s3cret"));
        assert!(value("x-note").starts_with("hello "));
    }
}
