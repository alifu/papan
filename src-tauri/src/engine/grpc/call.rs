//! One call, one task (plan 08 §5). The task opens the method through a [`Transport`],
//! reports response headers, logs and reports every inbound message, and ends with
//! exactly one `finished`, always last: the server's status and trailers, `CANCELLED`
//! when the client cancels (the response stream is dropped, which resets the HTTP/2
//! stream), or `DEADLINE_EXCEEDED` when the client timer fires.

use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use futures_util::future::BoxFuture;
use futures_util::stream::{BoxStream, StreamExt as _};
use prost::Message as _;
use prost_reflect::DynamicMessage;
use tokio::sync::oneshot;
use tokio::time::Instant;
use tonic::metadata::MetadataMap;
use tonic::{Code, Status};

use super::codec::{to_json_bounded, MAX_SHOWN_JSON_BYTES};
use super::metadata::shown;
use super::sink::{Finished, GrpcInbound};
use super::types::{GrpcEndBy, GrpcStatus};
use crate::engine::http::variables::now_ms;
use crate::engine::ws::log::Entry;
use crate::engine::ws::messages::Masker;
use crate::engine::ws::session::{lock_log, SharedLog};
use crate::engine::ws::types::{WsDirection, WsFrame, WsMessage};
use crate::error::{AppError, ErrorCode};

/// The request messages of a call; its end is the half-close.
pub type Requests = BoxStream<'static, DynamicMessage>;

/// A response stream: messages, then exactly one `Err` with the final status (`OK` with
/// the trailers as its metadata when the call succeeded).
pub type Responses = BoxStream<'static, Result<DynamicMessage, Status>>;

pub struct Opened {
    pub headers: MetadataMap,
    pub responses: Responses,
}

/// How a call reaches the server: tonic over our channel, or a fake in tests.
pub trait Transport: Send + 'static {
    /// Sends the request headers and `requests`; resolves with the response headers.
    fn open(self: Box<Self>, requests: Requests) -> BoxFuture<'static, Result<Opened, Status>>;
}

pub fn code_name(code: Code) -> &'static str {
    match code {
        Code::Ok => "OK",
        Code::Cancelled => "CANCELLED",
        Code::Unknown => "UNKNOWN",
        Code::InvalidArgument => "INVALID_ARGUMENT",
        Code::DeadlineExceeded => "DEADLINE_EXCEEDED",
        Code::NotFound => "NOT_FOUND",
        Code::AlreadyExists => "ALREADY_EXISTS",
        Code::PermissionDenied => "PERMISSION_DENIED",
        Code::ResourceExhausted => "RESOURCE_EXHAUSTED",
        Code::FailedPrecondition => "FAILED_PRECONDITION",
        Code::Aborted => "ABORTED",
        Code::OutOfRange => "OUT_OF_RANGE",
        Code::Unimplemented => "UNIMPLEMENTED",
        Code::Internal => "INTERNAL",
        Code::Unavailable => "UNAVAILABLE",
        Code::DataLoss => "DATA_LOSS",
        Code::Unauthenticated => "UNAUTHENTICATED",
    }
}

/// Logs `message` as JSON with secrets masked and returns the entry.
pub fn record(
    log: &SharedLog,
    mask: impl Fn(&str) -> String,
    direction: WsDirection,
    message: &DynamicMessage,
) -> WsMessage {
    let text = to_json_bounded(message, MAX_SHOWN_JSON_BYTES)
        .unwrap_or_else(|_| "<not shown>".to_string());
    lock_log(log).push(Entry {
        at_ms: now_ms(),
        direction,
        frame: WsFrame::Text,
        text: mask(&text),
        size: message.encoded_len(),
    })
}

pub struct CallTask {
    pub id: String,
    pub attempt: u32,
    pub transport: Box<dyn Transport>,
    pub requests: Requests,
    /// The client timer; the `grpc-timeout` header is the transport's.
    pub deadline: Option<Duration>,
    pub masker: Masker,
    pub log: SharedLog,
    pub inbound: Arc<dyn GrpcInbound>,
    /// A message, or the sender going away, cancels the call.
    pub cancel: oneshot::Receiver<()>,
}

enum Outcome {
    Cancelled,
    Deadline,
    Ended(Status),
}

#[derive(Default)]
struct Counters {
    messages: u64,
    bytes: u64,
}

async fn timer(deadline: Option<Duration>) {
    match deadline {
        Some(limit) => tokio::time::sleep(limit).await,
        None => std::future::pending().await,
    }
}

async fn drive(
    transport: Box<dyn Transport>,
    requests: Requests,
    deadline: Option<Duration>,
    cancel: oneshot::Receiver<()>,
    mut on_headers: impl FnMut(&MetadataMap),
    mut on_message: impl FnMut(&DynamicMessage),
) -> Outcome {
    let deadline = timer(deadline);
    tokio::pin!(deadline, cancel);
    let opened = tokio::select! {
        biased;
        _ = &mut cancel => return Outcome::Cancelled,
        () = &mut deadline => return Outcome::Deadline,
        opened = transport.open(requests) => opened,
    };
    let mut responses = match opened {
        Ok(opened) => {
            on_headers(&opened.headers);
            opened.responses
        }
        Err(status) => return Outcome::Ended(status),
    };
    loop {
        let step = tokio::select! {
            biased;
            _ = &mut cancel => return Outcome::Cancelled,
            () = &mut deadline => return Outcome::Deadline,
            step = responses.next() => step,
        };
        match step {
            Some(Ok(message)) => on_message(&message),
            Some(Err(status)) => return Outcome::Ended(status),
            None => {
                return Outcome::Ended(Status::unknown("The server ended the call with no status."))
            }
        }
    }
}

/// A status tonic made from a broken connection, not one the server sent.
fn transport_failure(status: &Status) -> Option<String> {
    std::error::Error::source(status).map(ToString::to_string)
}

/// tonic reports its own decode limit as `OUT_OF_RANGE`; the server sent no such status.
fn is_client_size_limit(status: &Status) -> bool {
    status.code() == Code::OutOfRange && status.message().contains("message length too large")
}

fn finished_of(outcome: Outcome, masker: &Masker, elapsed_ms: f64) -> Finished {
    let (status, by, error) = match outcome {
        Outcome::Cancelled => (
            Status::cancelled("The call was cancelled."),
            GrpcEndBy::Client,
            None,
        ),
        Outcome::Deadline => (
            Status::deadline_exceeded("The deadline was exceeded."),
            GrpcEndBy::Client,
            None,
        ),
        Outcome::Ended(status) if is_client_size_limit(&status) => (
            Status::resource_exhausted(status.message().to_string()),
            GrpcEndBy::Client,
            None,
        ),
        Outcome::Ended(status) => match transport_failure(&status) {
            None => (status, GrpcEndBy::Server, None),
            Some(detail) => {
                let error = AppError::new(ErrorCode::Network, "The connection failed.")
                    .with_detail(masker.mask(&detail));
                (status, GrpcEndBy::Error, Some(error))
            }
        },
    };
    let details = status.details();
    Finished {
        status: GrpcStatus {
            code: status.code() as u32,
            name: code_name(status.code()).to_string(),
            message: masker.mask(status.message()),
            details_base64: (!details.is_empty()).then(|| STANDARD.encode(details)),
        },
        trailers: shown(&status.metadata().clone().into_headers(), masker),
        by,
        elapsed_ms,
        error,
    }
}

/// Runs the call to its end. `finished` is reported exactly once, after everything else.
pub async fn run(task: CallTask) {
    let started = Instant::now();
    let mut counters = Counters::default();
    let (inbound, masker, log) = (&task.inbound, &task.masker, &task.log);
    let on_headers = |headers: &MetadataMap| {
        inbound.headers(shown(&headers.clone().into_headers(), masker));
    };
    let on_message = |message: &DynamicMessage| {
        let logged = record(
            log,
            |text| masker.mask(text),
            WsDirection::Received,
            message,
        );
        counters.messages += 1;
        counters.bytes += u64::from(logged.size);
        inbound.message(&logged);
    };
    let outcome = drive(
        task.transport,
        task.requests,
        task.deadline,
        task.cancel,
        on_headers,
        on_message,
    )
    .await;
    let finished = finished_of(outcome, masker, started.elapsed().as_secs_f64() * 1000.0);
    tracing::info!(
        call = %task.id,
        attempt = task.attempt,
        status = finished.status.code,
        by = ?finished.by,
        received = counters.messages,
        received_bytes = counters.bytes,
        "grpc call finished"
    );
    inbound.finished(finished);
}

#[cfg(test)]
#[path = "call_tests.rs"]
pub(crate) mod tests;
