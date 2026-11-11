//! Application logging: a daily-rolling file in the app log dir, plus stderr in debug builds.
//!
//! Never log secret variable values or credential headers; see [`redact`].

pub mod redact;

use std::path::Path;

use tracing::level_filters::LevelFilter;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

use crate::error::AppError;

const LOG_FILE_PREFIX: &str = "papan";
const LOG_FILE_SUFFIX: &str = "log";
/// Daily files kept before the oldest is deleted.
const MAX_LOG_FILES: usize = 14;

/// Installs the global subscriber. Keep the returned guard alive for the life of the
/// app: dropping it flushes and stops the background log writer.
pub fn init(log_dir: &Path) -> Result<WorkerGuard, AppError> {
    let appender = file_appender(log_dir)?;
    let (file_writer, guard) = tracing_appender::non_blocking(appender);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(file_writer);
    let stderr_layer = cfg!(debug_assertions)
        .then(|| tracing_subscriber::fmt::layer().with_writer(std::io::stderr));

    tracing_subscriber::registry()
        .with(file_layer.and_then(stderr_layer).with_filter(max_level()))
        .try_init()
        .map_err(|source| {
            AppError::internal("Could not start logging.").with_detail(source.to_string())
        })?;

    Ok(guard)
}

fn max_level() -> LevelFilter {
    if cfg!(debug_assertions) {
        LevelFilter::DEBUG
    } else {
        LevelFilter::INFO
    }
}

/// Creates `log_dir` if needed and returns a writer for `papan.<date>.log` files in it.
fn file_appender(log_dir: &Path) -> Result<RollingFileAppender, AppError> {
    std::fs::create_dir_all(log_dir)?;

    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILE_PREFIX)
        .filename_suffix(LOG_FILE_SUFFIX)
        .max_log_files(MAX_LOG_FILES)
        .build(log_dir)
        .map_err(|source| {
            AppError::internal("Could not open the log file.").with_detail(source.to_string())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn file_appender_writes_a_prefixed_log_file_in_a_new_directory() {
        let root = tempfile::tempdir().unwrap();
        let log_dir = root.path().join("nested").join("logs");

        let mut appender = file_appender(&log_dir).unwrap();
        appender.write_all(b"hello\n").unwrap();
        appender.flush().unwrap();

        let names: Vec<String> = std::fs::read_dir(&log_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 1);
        assert!(names[0].starts_with("papan.") && names[0].ends_with(".log"));
        let content = std::fs::read_to_string(log_dir.join(&names[0])).unwrap();
        assert_eq!(content, "hello\n");
    }

    #[test]
    fn file_appender_fails_with_io_code_when_directory_cannot_be_created() {
        let root = tempfile::tempdir().unwrap();
        let blocking_file = root.path().join("not-a-dir");
        std::fs::write(&blocking_file, b"x").unwrap();

        let result = file_appender(&blocking_file.join("logs"));

        assert_eq!(
            result.err().map(|e| e.code),
            Some(crate::error::ErrorCode::Io)
        );
    }
}
