//! One HTTP exchange: send, time it, stream the decoded body to a file up to a cap and map
//! failures to stable error codes (plan 03 §6).

use std::error::Error as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use reqwest::header::CONTENT_TYPE;
use reqwest::{Response, Version};
use tokio::io::AsyncWriteExt;

use super::build::{loggable_url, Prepared};
use super::client::ClientSelector;
use super::follow::{follow, Followed};
use super::types::{Header, ResponseCookie, ResponseMeta, Timing};
use crate::error::{AppError, ErrorCode};
use crate::store::types::HttpVersion;

/// Everything one send needs; owned so it can run as its own task.
#[derive(Debug)]
pub struct Exchange {
    pub request_id: String,
    /// Chooses the client of each hop (plan 03b §4.2).
    pub selector: ClientSelector,
    pub prepared: Prepared,
    /// `None` = no timeout.
    pub timeout: Option<Duration>,
    pub follow_redirects: bool,
    pub verify_tls: bool,
    /// The version `client` was built for; names the error when HTTP/2 is refused.
    pub http_version: HttpVersion,
    pub body_file: PathBuf,
    pub max_body_bytes: u64,
}

/// Deletes a partial body file unless the exchange completed (also when it is aborted).
struct PartialFile {
    path: PathBuf,
    complete: bool,
}

impl Drop for PartialFile {
    fn drop(&mut self) {
        if !self.complete {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn millis(elapsed: Duration) -> u32 {
    u32::try_from(elapsed.as_millis()).unwrap_or(u32::MAX)
}

pub async fn execute(exchange: Exchange) -> Result<ResponseMeta, AppError> {
    let started = Instant::now();
    let deadline = exchange.timeout.map(|timeout| started + timeout);
    let Followed {
        response,
        url,
        note,
    } = follow(&exchange, deadline).await?;
    let ttfb_ms = millis(started.elapsed());
    let meta = head(&exchange, &response, ttfb_ms);
    let status_text = match note {
        Some(note) => format!("{} ({note})", meta.status_text),
        None => meta.status_text,
    };
    let mut partial = PartialFile {
        path: exchange.body_file.clone(),
        complete: false,
    };
    let (size, truncated) = stream_body(response, &partial.path, exchange.max_body_bytes)
        .await
        .map_err(|error| error.with_url(&url))?;
    partial.complete = true;
    let total_ms = millis(started.elapsed());
    Ok(ResponseMeta {
        size_bytes: u32::try_from(size).unwrap_or(u32::MAX),
        truncated,
        total_ms,
        timing: exchange.selector.timing(started, ttfb_ms, total_ms),
        status_text,
        ..meta
    })
}

pub(super) fn timed_out() -> AppError {
    AppError::new(ErrorCode::Timeout, "The request timed out.")
}

fn version_text(version: Version) -> String {
    match version {
        Version::HTTP_09 => "HTTP/0.9",
        Version::HTTP_10 => "HTTP/1.0",
        Version::HTTP_2 => "HTTP/2",
        Version::HTTP_3 => "HTTP/3",
        _ => "HTTP/1.1",
    }
    .to_string()
}

fn head(exchange: &Exchange, response: &Response, ttfb_ms: u32) -> ResponseMeta {
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| Header {
            name: name.to_string(),
            value: String::from_utf8_lossy(value.as_bytes()).into_owned(),
        })
        .collect();
    let cookies = response
        .cookies()
        .map(|cookie| ResponseCookie {
            name: cookie.name().to_string(),
            value: cookie.value().to_string(),
            domain: cookie.domain().map(str::to_string),
            path: cookie.path().map(str::to_string),
            expires: cookie.expires().map(httpdate::fmt_http_date),
            http_only: cookie.http_only(),
            secure: cookie.secure(),
        })
        .collect();
    let status = response.status();
    ResponseMeta {
        request_id: exchange.request_id.clone(),
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or_default().to_string(),
        http_version: version_text(response.version()),
        url: response.url().to_string(),
        headers,
        cookies,
        content_type: response
            .headers()
            .get(CONTENT_TYPE)
            .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned()),
        size_bytes: 0,
        truncated: false,
        ttfb_ms,
        total_ms: ttfb_ms,
        // Measured once the body is read (plan 03b §6.4).
        timing: Timing::default(),
        tls_verified: exchange.verify_tls && response.url().scheme() == "https",
        unresolved: Vec::new(),
    }
}

/// A body failure, mapped once the host is known.
enum BodyError {
    Network(reqwest::Error),
    Io(std::io::Error),
}

impl BodyError {
    fn with_url(self, url: &url::Url) -> AppError {
        match self {
            BodyError::Network(error) => map_error(error, url),
            BodyError::Io(error) => AppError::from(error),
        }
    }
}

/// Writes the body to `path`, stopping at `cap` bytes. Returns (bytes written, truncated).
async fn stream_body(
    mut response: Response,
    path: &Path,
    cap: u64,
) -> Result<(u64, bool), BodyError> {
    let mut file = tokio::fs::File::create(path).await.map_err(BodyError::Io)?;
    let mut written: u64 = 0;
    let mut truncated = false;
    while let Some(chunk) = response.chunk().await.map_err(BodyError::Network)? {
        let room = usize::try_from(cap - written).unwrap_or(usize::MAX);
        let take = chunk.len().min(room);
        file.write_all(&chunk[..take])
            .await
            .map_err(BodyError::Io)?;
        written += take as u64;
        if take < chunk.len() || written == cap {
            truncated = take < chunk.len() || response.chunk().await.ok().flatten().is_some();
            break;
        }
    }
    file.flush().await.map_err(BodyError::Io)?;
    Ok((written, truncated))
}

/// The source chain, for logs. reqwest's own text would include the full URL (and so an
/// API key in its query): it is removed and the query-less URL added instead.
fn chain(error: reqwest::Error, url: &url::Url) -> String {
    let error = error.without_url();
    let text = format!("{error} ({})", loggable_url(url));
    std::iter::successors(error.source(), |source| (*source).source())
        .map(ToString::to_string)
        .fold(text, |text, source| format!("{text}: {source}"))
}

fn is_certificate_problem(detail: &str) -> bool {
    let lower = detail.to_lowercase();
    ["certificate", "unknownissuer", "invalidcertificate"]
        .iter()
        .any(|needle| lower.contains(needle))
}

pub fn map_error(error: reqwest::Error, url: &url::Url) -> AppError {
    let host = url.host_str().unwrap_or_default().to_string();
    let (timeout, builder, connect) = (error.is_timeout(), error.is_builder(), error.is_connect());
    let detail = chain(error, url);
    let mapped = if timeout {
        timed_out()
    } else if builder {
        AppError::invalid_input("The request could not be built.")
    } else if is_certificate_problem(&detail) {
        AppError::new(
            ErrorCode::Network,
            "The server's TLS certificate could not be verified.",
        )
    } else if connect {
        AppError::new(ErrorCode::Network, format!("Could not connect to {host}."))
    } else {
        AppError::new(
            ErrorCode::Network,
            format!("The exchange with {host} failed."),
        )
    };
    mapped.with_detail(detail)
}
