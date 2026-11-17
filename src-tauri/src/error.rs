//! The single error type that crosses the IPC boundary.
//!
//! Wire shape: `{ "code": "...", "message": "...", "detail"?: "..." }`.
//! - `code` is a stable machine string; the frontend may branch on it.
//! - `message` is safe and meaningful to show to the user.
//! - `detail` is technical context for logs. Never put secrets in it.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Stable machine-readable error codes. Renaming a variant is a breaking IPC change.
// `Deserialize` because history entries store the code of a failed send.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The caller supplied something invalid (bad file, bad field, bad argument).
    InvalidInput,
    /// A requested resource does not exist.
    NotFound,
    /// A filesystem or other I/O operation failed.
    Io,
    /// An unexpected failure inside the app.
    Internal,
    /// A request could not reach the server (DNS, connect, TLS).
    Network,
    /// A request took longer than its timeout.
    Timeout,
    /// A request was cancelled by the user.
    Cancelled,
}

/// Error returned by every fallible command: `Result<T, AppError>`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Type)]
#[error("{message}")]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
        }
    }

    /// Returns a copy of this error carrying technical detail for the logs.
    #[must_use]
    pub fn with_detail(self, detail: impl Into<String>) -> Self {
        Self {
            detail: Some(detail.into()),
            ..self
        }
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }
}

impl From<std::io::Error> for AppError {
    fn from(source: std::io::Error) -> Self {
        Self::new(ErrorCode::Io, "A file operation failed.").with_detail(source.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serialises_code_and_message_and_omits_absent_detail() {
        let error = AppError::not_found("Collection not found.");

        let value = serde_json::to_value(&error).unwrap();

        assert_eq!(
            value,
            json!({ "code": "not_found", "message": "Collection not found." })
        );
    }

    #[test]
    fn serialises_detail_when_present() {
        let error = AppError::invalid_input("Invalid request file.").with_detail("line 3: url");

        let value = serde_json::to_value(&error).unwrap();

        assert_eq!(
            value,
            json!({
                "code": "invalid_input",
                "message": "Invalid request file.",
                "detail": "line 3: url"
            })
        );
    }

    #[test]
    fn every_code_serialises_to_its_stable_string() {
        let cases = [
            (ErrorCode::InvalidInput, "invalid_input"),
            (ErrorCode::NotFound, "not_found"),
            (ErrorCode::Io, "io"),
            (ErrorCode::Internal, "internal"),
            (ErrorCode::Network, "network"),
            (ErrorCode::Timeout, "timeout"),
            (ErrorCode::Cancelled, "cancelled"),
        ];

        for (code, expected) in cases {
            assert_eq!(serde_json::to_value(code).unwrap(), json!(expected));
        }
    }

    #[test]
    fn io_error_maps_to_io_code_with_source_in_detail() {
        let source = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");

        let error = AppError::from(source);

        assert_eq!(error.code, ErrorCode::Io);
        assert_eq!(error.message, "A file operation failed.");
        assert_eq!(error.detail.as_deref(), Some("no such file"));
    }

    #[test]
    fn with_detail_keeps_code_and_message() {
        let error = AppError::internal("Something went wrong.").with_detail("ctx");

        assert_eq!(error.code, ErrorCode::Internal);
        assert_eq!(error.to_string(), "Something went wrong.");
    }
}
