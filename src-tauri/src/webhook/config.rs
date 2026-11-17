//! Listener configs: `<app data>/webhooks.json`, validated on every read and write
//! (plan 07 §6).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::types::WebhookConfig;
use crate::error::AppError;
use crate::store::fsio;

pub const MAX_LISTENERS: usize = 20;
pub const DEFAULT_MAX_BODY: u32 = 1024 * 1024;
pub const DEFAULT_MAX_CAPTURES: u32 = 200;
const MIN_BODY: u32 = 1024;
const MAX_BODY: u32 = 10 * 1024 * 1024;
const MAX_CAPTURES: u32 = 1000;
const MAX_RESPONSE_BODY: usize = 1024 * 1024;
const MAX_RESPONSE_HEADERS: usize = 50;
const MAX_NAME: usize = 64;
const MAX_PATH: usize = 256;
const FILE_NAME: &str = "webhooks.json";

fn invalid(message: impl Into<String>) -> AppError {
    AppError::invalid_input(message)
}

/// Fills the zero "use the default" limits so a new listener needs only a name and path.
pub fn normalize(mut config: WebhookConfig) -> WebhookConfig {
    if config.max_body_bytes == 0 {
        config.max_body_bytes = DEFAULT_MAX_BODY;
    }
    if config.max_captures == 0 {
        config.max_captures = DEFAULT_MAX_CAPTURES;
    }
    config.name = config.name.trim().to_string();
    config
}

pub fn validate(config: &WebhookConfig) -> Result<(), AppError> {
    let name_len = config.name.chars().count();
    if name_len == 0 || name_len > MAX_NAME {
        return Err(invalid("The name must be 1 to 64 characters."));
    }
    if config.port != 0 && !(1024..=65535).contains(&config.port) {
        return Err(invalid(
            "The port must be 0 (any free port) or 1024 to 65535.",
        ));
    }
    validate_path(&config.path)?;
    validate_response(config)?;
    if !(MIN_BODY..=MAX_BODY).contains(&config.max_body_bytes) {
        return Err(invalid("The body limit must be 1 KiB to 10 MiB."));
    }
    if !(1..=MAX_CAPTURES).contains(&config.max_captures) {
        return Err(invalid("The capture limit must be 1 to 1000."));
    }
    Ok(())
}

fn validate_path(path: &str) -> Result<(), AppError> {
    let bad_char = |c: char| c.is_whitespace() || c.is_control() || c == '?' || c == '#';
    let dot_dot = path.split('/').any(|segment| segment == "..");
    if !path.starts_with('/') || path.len() > MAX_PATH || dot_dot || path.contains(bad_char) {
        return Err(invalid(
            "The path must start with / and have no spaces, ?, # or .. (256 characters at most).",
        ));
    }
    Ok(())
}

fn validate_response(config: &WebhookConfig) -> Result<(), AppError> {
    let response = &config.response;
    if !(200..=599).contains(&response.status) {
        return Err(invalid("The response status must be 200 to 599."));
    }
    if response.body.len() > MAX_RESPONSE_BODY {
        return Err(invalid("The response body must be 1 MiB or less."));
    }
    if response.headers.len() > MAX_RESPONSE_HEADERS {
        return Err(invalid("At most 50 response headers are allowed."));
    }
    for header in &response.headers {
        let name = http::HeaderName::from_bytes(header.name.as_bytes())
            .map_err(|_| invalid(format!("\"{}\" is not a valid header name.", header.name)))?;
        if name == http::header::CONTENT_LENGTH || name == http::header::TRANSFER_ENCODING {
            return Err(invalid(format!("The {name} header is set automatically.")));
        }
        if http::HeaderValue::from_str(&header.value).is_err() {
            return Err(invalid(format!("The value of {name} is not valid.")));
        }
    }
    Ok(())
}

#[derive(Serialize, Deserialize, Default)]
struct ConfigFile {
    listeners: Vec<serde_json::Value>,
}

/// `webhooks.json` in the app data directory.
#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn new(app_data: &Path) -> Self {
        Self {
            path: app_data.join(FILE_NAME),
        }
    }

    /// Valid listeners only: a bad or surplus entry is dropped with a warning, and an
    /// unreadable file is an empty list (it is rewritten on the next save).
    pub fn load(&self) -> Vec<WebhookConfig> {
        let Ok(bytes) = fsio::read_bounded(&self.path) else {
            return Vec::new();
        };
        let file: ConfigFile = serde_json::from_slice(&bytes).unwrap_or_else(|source| {
            tracing::warn!(%source, "webhooks.json is not valid; ignoring it");
            ConfigFile::default()
        });
        let mut loaded: Vec<WebhookConfig> = Vec::new();
        for value in file.listeners {
            match serde_json::from_value::<WebhookConfig>(value) {
                Ok(config)
                    if validate(&config).is_ok() && !loaded.iter().any(|c| c.id == config.id) =>
                {
                    loaded.push(config);
                }
                _ => tracing::warn!("dropping an invalid webhook listener from webhooks.json"),
            }
        }
        loaded.truncate(MAX_LISTENERS);
        loaded
    }

    pub fn save(&self, configs: &[WebhookConfig]) -> Result<(), AppError> {
        let listeners = configs
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|source| {
                AppError::internal("Could not save the webhook listeners.")
                    .with_detail(source.to_string())
            })?;
        let text = serde_json::to_string_pretty(&ConfigFile { listeners }).map_err(|source| {
            AppError::internal("Could not save the webhook listeners.")
                .with_detail(source.to_string())
        })?;
        fsio::write_atomic(&self.path, text.as_bytes()).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::types::KeyValue;
    use crate::webhook::fixtures::sample;

    fn header(name: &str, value: &str) -> KeyValue {
        KeyValue {
            name: name.into(),
            value: value.into(),
            enabled: true,
        }
    }

    #[test]
    fn accepts_the_sample() {
        assert!(validate(&sample()).is_ok());
    }

    #[test]
    fn rejects_each_invalid_field() {
        type Edit = fn(&mut WebhookConfig);
        let cases: Vec<(&str, Edit)> = vec![
            ("empty name", |c| c.name = String::new()),
            ("long name", |c| c.name = "x".repeat(65)),
            ("low port", |c| c.port = 80),
            ("high port", |c| c.port = 70000),
            ("no slash", |c| c.path = "hooks".into()),
            ("dot dot", |c| c.path = "/a/../b".into()),
            ("query", |c| c.path = "/a?b".into()),
            ("fragment", |c| c.path = "/a#b".into()),
            ("space", |c| c.path = "/a b".into()),
            ("long path", |c| c.path = format!("/{}", "a".repeat(256))),
            ("status 199", |c| c.response.status = 199),
            ("status 600", |c| c.response.status = 600),
            ("big response", |c| {
                c.response.body = "x".repeat(1024 * 1024 + 1)
            }),
            ("bad header name", |c| {
                c.response.headers = vec![header("a b", "1")]
            }),
            ("content-length", |c| {
                c.response.headers = vec![header("Content-Length", "1")]
            }),
            ("transfer-encoding", |c| {
                c.response.headers = vec![header("transfer-encoding", "chunked")]
            }),
            ("bad header value", |c| {
                c.response.headers = vec![header("X-A", "a\nb")]
            }),
            ("many headers", |c| {
                c.response.headers = (0..51).map(|i| header(&format!("x-{i}"), "1")).collect()
            }),
            ("small body cap", |c| c.max_body_bytes = 100),
            ("big body cap", |c| c.max_body_bytes = MAX_BODY + 1),
            ("zero captures", |c| c.max_captures = 0),
            ("many captures", |c| c.max_captures = 1001),
        ];
        for (label, edit) in cases {
            let mut config = sample();
            edit(&mut config);
            assert!(validate(&config).is_err(), "{label} should be rejected");
        }
    }

    #[test]
    fn normalize_fills_zero_limits() {
        let mut config = sample();
        config.max_body_bytes = 0;
        config.max_captures = 0;
        config.name = "  Spaced ".into();
        let config = normalize(config);
        assert_eq!(config.max_body_bytes, DEFAULT_MAX_BODY);
        assert_eq!(config.max_captures, DEFAULT_MAX_CAPTURES);
        assert_eq!(config.name, "Spaced");
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(dir.path());
        store.save(&[sample()]).unwrap();
        assert_eq!(store.load(), vec![sample()]);
    }

    #[test]
    fn load_drops_invalid_and_duplicate_entries() {
        let dir = tempfile::tempdir().unwrap();
        let mut bad = sample();
        bad.id = "other".into();
        bad.path = "nope".into();
        let good = serde_json::to_value(sample()).unwrap();
        let file = serde_json::json!({ "listeners": [good.clone(), good, serde_json::to_value(bad).unwrap(), 5] });
        std::fs::write(dir.path().join(FILE_NAME), file.to_string()).unwrap();
        assert_eq!(ConfigStore::new(dir.path()).load(), vec![sample()]);
    }

    #[test]
    fn load_of_garbage_or_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(dir.path());
        assert!(store.load().is_empty());
        std::fs::write(dir.path().join(FILE_NAME), "not json").unwrap();
        assert!(store.load().is_empty());
    }
}
