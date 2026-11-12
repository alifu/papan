//! One HTTP exchange: send, time it, stream the decoded body to a file up to a cap and map
//! failures to stable error codes (plan 03 §6).

use std::error::Error as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, LOCATION};
use reqwest::multipart::{Form, Part};
use reqwest::{Client, RequestBuilder, Response, Version};
use tokio::io::AsyncWriteExt;

use super::build::{loggable_url, PartValue, Prepared, PreparedBody};
use super::redirect::{next_hop, MAX_REDIRECTS};
use super::types::{Header, ResponseCookie, ResponseMeta};
use crate::error::{AppError, ErrorCode};

/// Everything one send needs; owned so it can run as its own task.
#[derive(Debug)]
pub struct Exchange {
    pub request_id: String,
    pub client: Client,
    pub prepared: Prepared,
    /// `None` = no timeout.
    pub timeout: Option<Duration>,
    pub follow_redirects: bool,
    pub verify_tls: bool,
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
    let (response, url) = follow(&exchange, deadline).await?;
    let ttfb_ms = millis(started.elapsed());
    let meta = head(&exchange, &response, ttfb_ms);
    let mut partial = PartialFile {
        path: exchange.body_file.clone(),
        complete: false,
    };
    let (size, truncated) = stream_body(response, &partial.path, exchange.max_body_bytes)
        .await
        .map_err(|error| error.with_url(&url))?;
    partial.complete = true;
    Ok(ResponseMeta {
        size_bytes: u32::try_from(size).unwrap_or(u32::MAX),
        truncated,
        total_ms: millis(started.elapsed()),
        ..meta
    })
}

fn timed_out() -> AppError {
    AppError::new(ErrorCode::Timeout, "The request timed out.")
}

/// Sends the request and, when allowed, follows up to [`MAX_REDIRECTS`] redirects.
/// Returns the final response and the URL it came from.
async fn follow(
    exchange: &Exchange,
    deadline: Option<Instant>,
) -> Result<(Response, url::Url), AppError> {
    let mut current = exchange.prepared.clone();
    let mut redirects = 0;
    loop {
        let remaining = match deadline {
            Some(deadline) => Some(
                deadline
                    .checked_duration_since(Instant::now())
                    .filter(|left| !left.is_zero())
                    .ok_or_else(timed_out)?,
            ),
            None => None,
        };
        let builder = with_body(request(&exchange.client, &current, remaining), &current).await?;
        let response = builder
            .send()
            .await
            .map_err(|error| map_error(error, &current.url))?;
        let next = exchange
            .follow_redirects
            .then(|| {
                next_hop(
                    &current,
                    response.status(),
                    response.headers().get(LOCATION),
                )
            })
            .flatten();
        let Some(next) = next else {
            return Ok((response, current.url));
        };
        if redirects == MAX_REDIRECTS {
            return Err(
                AppError::new(ErrorCode::Network, "The server redirected too many times.")
                    .with_detail(loggable_url(&current.url)),
            );
        }
        redirects += 1;
        current = next;
    }
}

fn request(client: &Client, prepared: &Prepared, timeout: Option<Duration>) -> RequestBuilder {
    let builder = prepared.headers.iter().fold(
        client.request(prepared.method.clone(), prepared.url.clone()),
        |builder, (name, value)| builder.header(name, value),
    );
    match timeout {
        Some(timeout) => builder.timeout(timeout),
        None => builder,
    }
}

fn file_error(path: &Path, source: &std::io::Error) -> AppError {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    AppError::invalid_input(format!("Could not read the file to upload: {name}"))
        .with_detail(source.to_string())
}

async fn open(path: &Path) -> Result<(tokio::fs::File, u64), AppError> {
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|source| file_error(path, &source))?;
    let length = file
        .metadata()
        .await
        .map_err(|source| file_error(path, &source))?
        .len();
    Ok((file, length))
}

async fn with_body(
    builder: RequestBuilder,
    prepared: &Prepared,
) -> Result<RequestBuilder, AppError> {
    Ok(match &prepared.body {
        PreparedBody::None => builder,
        PreparedBody::Bytes(bytes) => builder.body(bytes.clone()),
        PreparedBody::File(path) => {
            let (file, length) = open(path).await?;
            let has_length = prepared
                .headers
                .iter()
                .any(|(name, _)| name == CONTENT_LENGTH);
            let builder = if has_length {
                builder
            } else {
                builder.header(CONTENT_LENGTH, length)
            };
            builder.body(reqwest::Body::from(file))
        }
        PreparedBody::Multipart(parts) => {
            let mut form = Form::new();
            for part in parts {
                form = match &part.value {
                    PartValue::Text(text) => form.text(part.name.clone(), text.clone()),
                    PartValue::File(path) => {
                        let (file, length) = open(path).await?;
                        let name = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let body = Part::stream_with_length(reqwest::Body::from(file), length)
                            .file_name(name);
                        form.part(part.name.clone(), body)
                    }
                };
            }
            builder.multipart(form)
        }
    })
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
