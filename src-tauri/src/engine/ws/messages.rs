//! Outgoing messages (plan 07 §5): a template resolved with the scopes of its request
//! becomes an [`Outbound`], whose `shown` text (secrets masked) is what the log keeps.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;

use super::types::WsOutgoing;
use crate::engine::http::resolve::Resolved;
use crate::error::AppError;
use crate::store::types::{Body, RequestContent, WsFormat};

/// The largest message that is sent (the inbound limit is the same).
pub const MAX_OUTGOING_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payload {
    Text(String),
    Binary(Vec<u8>),
}

/// A message ready for the wire. `shown` is the text logged for it: the message itself
/// (masked) for text, base64 for binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outbound {
    pub payload: Payload,
    pub shown: String,
}

impl Outbound {
    /// A text message logged as it is (no variables, no secrets): for protocol drivers.
    pub fn text(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            shown: text.clone(),
            payload: Payload::Text(text),
        }
    }

    pub fn binary(bytes: Vec<u8>) -> Self {
        Self {
            shown: STANDARD.encode(&bytes),
            payload: Payload::Binary(bytes),
        }
    }

    pub fn size(&self) -> usize {
        match &self.payload {
            Payload::Text(text) => text.len(),
            Payload::Binary(bytes) => bytes.len(),
        }
    }
}

/// The message text as a request body, so that it is resolved like any other text of a
/// request (scopes, secrets, dynamic variables) by `resolve_for`.
pub fn outgoing_content(outgoing: &WsOutgoing) -> RequestContent {
    RequestContent {
        body: Body::Text {
            text: outgoing.text.clone(),
        },
        ..RequestContent::default()
    }
}

fn not_defined(names: &[String]) -> AppError {
    let names: Vec<String> = names.iter().map(|n| format!("`{n}`")).collect();
    AppError::invalid_input(format!(
        "Not defined: {}. Select an environment or define it.",
        names.join(", ")
    ))
}

/// Checks the resolved text against `outgoing.frame` and builds the message.
pub fn finish_outgoing(outgoing: &WsOutgoing, resolved: &Resolved) -> Result<Outbound, AppError> {
    if !resolved.unresolved.is_empty() {
        return Err(not_defined(&resolved.unresolved));
    }
    let Body::Text { text } = &resolved.content.body else {
        return Err(AppError::internal("Could not prepare the message."));
    };
    let outbound = match outgoing.frame {
        WsFormat::Text => Outbound {
            shown: resolved.redact(text),
            payload: Payload::Text(text.clone()),
        },
        WsFormat::Json => {
            serde_json::from_str::<serde::de::IgnoredAny>(text).map_err(|_| {
                AppError::invalid_input("The message is not valid JSON after resolving variables.")
            })?;
            Outbound {
                shown: resolved.redact(text),
                payload: Payload::Text(text.clone()),
            }
        }
        WsFormat::Binary => Outbound::binary(decode_base64(text)?),
    };
    if outbound.size() > MAX_OUTGOING_BYTES {
        return Err(AppError::invalid_input(
            "The message is larger than 16 MiB.",
        ));
    }
    Ok(outbound)
}

fn decode_base64(text: &str) -> Result<Vec<u8>, AppError> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    STANDARD
        .decode(compact)
        .map_err(|_| AppError::invalid_input("The binary message is not valid base64."))
}

/// Masks secret values in text that came from the network or from the user.
#[derive(Debug, Clone, Default)]
pub struct Masker(Resolved);

impl Masker {
    pub fn new(secret_values: Vec<String>) -> Self {
        Self(Resolved {
            secret_values,
            ..Resolved::default()
        })
    }

    pub fn mask(&self, text: &str) -> String {
        self.0.redact(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    fn outgoing(frame: WsFormat, text: &str) -> WsOutgoing {
        WsOutgoing {
            frame,
            text: text.into(),
        }
    }

    /// `outgoing` as `resolve_for` would return it, with `secrets` used.
    fn resolved(outgoing: &WsOutgoing, text: &str, secrets: &[&str]) -> Resolved {
        Resolved {
            content: RequestContent {
                body: Body::Text { text: text.into() },
                ..outgoing_content(outgoing)
            },
            secret_values: secrets.iter().map(|s| s.to_string()).collect(),
            ..Resolved::default()
        }
    }

    fn finish(frame: WsFormat, text: &str, secrets: &[&str]) -> Result<Outbound, AppError> {
        let out = outgoing(frame, text);
        finish_outgoing(&out, &resolved(&out, text, secrets))
    }

    #[test]
    fn text_is_sent_as_is_and_masked_in_the_log() {
        let sent = finish(WsFormat::Text, "token=s3cret!", &["s3cret!"]).unwrap();

        assert_eq!(sent.payload, Payload::Text("token=s3cret!".into()));
        assert!(!sent.shown.contains("s3cret!"), "{}", sent.shown);
    }

    #[test]
    fn json_is_validated_after_resolution() {
        assert!(finish(WsFormat::Json, r#"{"a": 1}"#, &[]).is_ok());

        let error = finish(WsFormat::Json, r#"{"a": "#, &[]).unwrap_err();

        assert_eq!(error.code, ErrorCode::InvalidInput);
        assert!(error.message.contains("JSON"), "{}", error.message);
    }

    #[test]
    fn binary_is_decoded_from_base64_after_resolution_and_logged_as_base64() {
        let sent = finish(WsFormat::Binary, "AAEC\n", &[]).unwrap();

        assert_eq!(sent.payload, Payload::Binary(vec![0, 1, 2]));
        assert_eq!(sent.shown, "AAEC");
        assert!(finish(WsFormat::Binary, "not base64!", &[]).is_err());
    }

    #[test]
    fn undefined_variables_refuse_the_send() {
        let out = outgoing(WsFormat::Text, "{{x}}");
        let unresolved = Resolved {
            unresolved: vec!["x".into()],
            ..resolved(&out, "{{x}}", &[])
        };

        let error = finish_outgoing(&out, &unresolved).unwrap_err();

        assert!(error.message.contains("`x`"), "{}", error.message);
    }

    #[test]
    fn a_message_over_the_limit_is_refused() {
        let text = "a".repeat(MAX_OUTGOING_BYTES + 1);

        assert!(finish(WsFormat::Text, &text, &[]).is_err());
    }

    #[test]
    fn protocol_drivers_build_messages_without_templates() {
        assert_eq!(Outbound::text("hi").shown, "hi");
        assert_eq!(Outbound::binary(vec![1, 2, 3]).shown, "AQID");
        assert_eq!(Outbound::binary(vec![1, 2, 3]).size(), 3);
    }

    #[test]
    fn the_masker_hides_secrets() {
        let masker = Masker::new(vec!["hunter2".into()]);

        assert!(!masker.mask("pw hunter2 here").contains("hunter2"));
    }
}
