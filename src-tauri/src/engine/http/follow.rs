//! The hops of one send (plan 03b §6): redirects (see `redirect`) and the single Digest
//! answer to a `401`. Bodies are rebuilt from `Prepared` for every request, so files are
//! re-streamed.

use std::path::Path;
use std::time::{Duration, Instant};

use reqwest::header::{HeaderValue, AUTHORIZATION, CONTENT_LENGTH, LOCATION, WWW_AUTHENTICATE};
use reqwest::multipart::{Form, Part};
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use url::{Position, Url};

use super::build::{loggable_url, PartValue, Prepared, PreparedBody};
use super::digest::{self, Challenge, DigestCredentials, Offer};
use super::redirect::{next_hop, MAX_REDIRECTS};
use super::send::{map_error, timed_out, Exchange};
use crate::error::{AppError, ErrorCode};
use crate::store::types::HttpVersion;

/// Shown after the status text when a Digest `401` could not be answered.
pub const UNSUPPORTED_DIGEST_NOTE: &str = "Digest challenge not supported";
const NO_HTTP2: &str = "The server does not support HTTP/2.";

/// The final response, the URL it came from and an optional note for the status line.
pub struct Followed {
    pub response: Response,
    pub url: Url,
    pub note: Option<&'static str>,
}

/// Sends the request and, when allowed, follows up to [`MAX_REDIRECTS`] redirects.
pub async fn follow(exchange: &Exchange, deadline: Option<Instant>) -> Result<Followed, AppError> {
    let mut current = exchange.prepared.clone();
    let mut redirects = 0;
    loop {
        let (response, note) = send_answering(exchange, &current, deadline).await?;
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
            return Ok(Followed {
                response,
                url: current.url,
                note,
            });
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

/// One hop: sends `prepared` and answers a Digest challenge once. Only a Digest
/// challenge is answered; a `Basic` one never receives the Digest password.
async fn send_answering(
    exchange: &Exchange,
    prepared: &Prepared,
    deadline: Option<Instant>,
) -> Result<(Response, Option<&'static str>), AppError> {
    let response = send_once(exchange, prepared, deadline).await?;
    let Some(credentials) = prepared
        .digest
        .as_ref()
        .filter(|_| response.status() == StatusCode::UNAUTHORIZED)
    else {
        return Ok((response, None));
    };
    let offer = digest::select(
        response
            .headers()
            .get_all(WWW_AUTHENTICATE)
            .iter()
            .filter_map(|value| value.to_str().ok()),
    );
    match offer {
        Offer::NoDigest => Ok((response, None)),
        Offer::Unsupported => Ok((response, Some(UNSUPPORTED_DIGEST_NOTE))),
        Offer::Supported(challenge) => {
            drop(response);
            let answered = answered(prepared, &challenge, credentials)?;
            Ok((send_once(exchange, &answered, deadline).await?, None))
        }
    }
}

/// `prepared` with the Digest `Authorization` in place of any other.
fn answered(
    prepared: &Prepared,
    challenge: &Challenge,
    credentials: &DigestCredentials,
) -> Result<Prepared, AppError> {
    let uri = &prepared.url[Position::BeforePath..Position::AfterQuery];
    let text = digest::authorization(
        challenge,
        credentials,
        prepared.method.as_str(),
        uri,
        &digest::new_cnonce()?,
    );
    let mut value = HeaderValue::from_str(&text).map_err(|_| {
        AppError::invalid_input("The Digest username contains characters that cannot be sent.")
    })?;
    value.set_sensitive(true);
    let headers = prepared
        .headers
        .iter()
        .filter(|(name, _)| *name != AUTHORIZATION)
        .cloned()
        .chain([(AUTHORIZATION, value)])
        .collect();
    Ok(Prepared {
        headers,
        ..prepared.clone()
    })
}

async fn send_once(
    exchange: &Exchange,
    prepared: &Prepared,
    deadline: Option<Instant>,
) -> Result<Response, AppError> {
    let remaining = match deadline {
        Some(deadline) => Some(
            deadline
                .checked_duration_since(Instant::now())
                .filter(|left| !left.is_zero())
                .ok_or_else(timed_out)?,
        ),
        None => None,
    };
    let client = exchange.selector.for_url(&prepared.url).await?;
    let builder = with_body(request(&client, prepared, remaining), prepared).await?;
    builder
        .send()
        .await
        .map_err(|error| version_error(map_error(error, &prepared.url), exchange.http_version))
}

/// What a refused HTTP/2 looks like in the error chain: the server's `no_application_protocol`
/// alert or rustls finding no common ALPN, or an HTTP/1 reply read as an h2 frame header
/// after the prior-knowledge preface (its length is far over the frame size limit).
const HTTP2_REFUSED: [&str; 3] = [
    "noapplicationprotocol",
    "peer doesn't support any known protocol",
    "http2 error: connection error detected: frame with invalid size",
];

/// A forced HTTP/2 that the server cannot speak: no common ALPN over TLS, or an HTTP/1
/// reply to the prior-knowledge preface over cleartext.
fn version_error(error: AppError, version: HttpVersion) -> AppError {
    let detail = error.detail.as_deref().unwrap_or_default().to_lowercase();
    let refused = HTTP2_REFUSED.iter().any(|needle| detail.contains(needle));
    if version == HttpVersion::Http2 && error.code == ErrorCode::Network && refused {
        AppError {
            message: NO_HTTP2.to_string(),
            ..error
        }
    } else {
        error
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

#[cfg(test)]
mod tests {
    use super::*;

    fn network(detail: &str) -> AppError {
        AppError::new(ErrorCode::Network, "The exchange with h failed.").with_detail(detail)
    }

    #[test]
    fn only_a_refused_http2_is_reported_as_missing_http2_support() {
        let refused = [
            "client error (Connect): received fatal alert: NoApplicationProtocol",
            "client error (Connect): peer doesn't support any known protocol",
            "http2 error: connection error detected: frame with invalid size",
        ];
        let other = [
            "http2 error: stream error received: refused stream",
            "http2 error: connection error received: not a result of an error",
            "client error (SendRequest): connection error: broken pipe",
        ];

        for detail in refused {
            let error = version_error(network(detail), HttpVersion::Http2);
            assert_eq!(error.message, NO_HTTP2, "{detail}");
        }
        for detail in other {
            let error = version_error(network(detail), HttpVersion::Http2);
            assert_ne!(error.message, NO_HTTP2, "{detail}");
        }
        let auto = version_error(network(refused[0]), HttpVersion::Auto);
        assert_ne!(auto.message, NO_HTTP2);
    }
}
