//! The HTTP/2 channel of a call (plan 08 §5, §11 D5): a tonic `Endpoint` over our own
//! connector, which is M7's transport (TCP or a proxy `CONNECT` tunnel, then rustls with
//! ALPN `h2`, `verify_tls` and the client certificate). tonic has no TLS feature here, so
//! an `https` origin is simply carried as `:scheme`. [`TonicTransport`] is the call
//! transport over such a channel.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_util::future::BoxFuture;
use futures_util::stream::{self, StreamExt as _};
use http::uri::PathAndQuery;
use http::Uri;
use hyper_util::rt::TokioIo;
use prost_reflect::MethodDescriptor;
use tonic::client::Grpc;
use tonic::metadata::MetadataMap;
use tonic::transport::{Channel, Endpoint};
use tonic::{Code, Request, Status, Streaming};
use url::Host;

use super::call::{Opened, Requests, Transport};
use super::codec::{DynamicCodec, MAX_MESSAGE_BYTES, MAX_RESPONSE_BYTES};
use super::connect::GrpcCallSpec;
use super::schema::path_of;
use crate::engine::ws::transport::{connect_stream, BoxedIo, ProxyHop, TlsOptions};
use crate::error::{AppError, ErrorCode};

const ALPN_H2: &[u8] = b"h2";

/// Opens the stream of every connection of one channel.
#[derive(Clone)]
struct Connector {
    host: Host<String>,
    port: u16,
    proxy: Option<ProxyHop>,
    tls: Option<TlsOptions>,
}

impl tower_service::Service<Uri> for Connector {
    type Response = TokioIo<BoxedIo>;
    type Error = AppError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, AppError>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), AppError>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _uri: Uri) -> Self::Future {
        let target = self.clone();
        Box::pin(async move {
            let host = match &target.host {
                Host::Domain(name) => Host::Domain(name.as_str()),
                Host::Ipv4(address) => Host::Ipv4(*address),
                Host::Ipv6(address) => Host::Ipv6(*address),
            };
            let alpn: &[&[u8]] = &[ALPN_H2];
            let (proxy, tls) = (target.proxy.as_ref(), target.tls.as_ref());
            let io = connect_stream(&host, target.port, proxy, tls, alpn).await?;
            Ok(TokioIo::new(io))
        })
    }
}

/// The [`AppError`] our connector raised, found below tonic's wrapping.
fn connector_error(error: &(dyn std::error::Error + 'static)) -> Option<AppError> {
    let mut current = Some(error);
    while let Some(layer) = current {
        if let Some(found) = layer.downcast_ref::<AppError>() {
            return Some(found.clone());
        }
        current = layer.source();
    }
    None
}

fn connect_error(error: &tonic::transport::Error) -> AppError {
    connector_error(error).unwrap_or_else(|| {
        let detail = std::error::Error::source(error)
            .map(ToString::to_string)
            .unwrap_or_else(|| error.to_string());
        AppError::new(ErrorCode::Network, "Could not connect to the server.").with_detail(detail)
    })
}

/// The connect timeout when the request sets none.
pub const DEFAULT_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Connects to the server of `spec` (HTTP/2 handshake included), within its timeout (or
/// [`DEFAULT_CONNECT_TIMEOUT`]).
pub async fn connect(spec: &GrpcCallSpec) -> Result<Channel, AppError> {
    let invalid = || AppError::invalid_input(super::connect::INVALID_URL_MESSAGE);
    let host = spec.url.host().ok_or_else(invalid)?.to_owned();
    let port = spec.url.port_or_known_default().ok_or_else(invalid)?;
    let origin = format!("{}://{}:{port}", spec.url.scheme(), host);
    let endpoint = Endpoint::from_shared(origin)
        .map_err(|_| invalid())?
        .user_agent(spec.user_agent.clone())
        .map_err(|_| AppError::internal("The user agent is not valid."))?;
    let connector = Connector {
        host,
        port,
        proxy: spec.proxy.clone(),
        tls: spec.tls.clone(),
    };
    let attempt = async {
        let connected = endpoint.connect_with_connector(connector).await;
        connected.map_err(|error| connect_error(&error))
    };
    let limit = spec.timeout.unwrap_or(DEFAULT_CONNECT_TIMEOUT);
    tokio::time::timeout(limit, attempt)
        .await
        .unwrap_or_else(|_| {
            Err(AppError::new(
                ErrorCode::Timeout,
                "The connection timed out.",
            ))
        })
}

/// A client on `channel` with the message size caps of plan 08 §11 D7.
pub fn client(channel: Channel) -> Grpc<Channel> {
    Grpc::new(channel)
        .max_decoding_message_size(MAX_RESPONSE_BYTES)
        .max_encoding_message_size(MAX_MESSAGE_BYTES)
}

/// Waits until `grpc` can take a request; a dead channel becomes a status.
pub async fn ready(grpc: &mut Grpc<Channel>) -> Result<(), Status> {
    grpc.ready()
        .await
        .map_err(|error| Status::from_error(Box::new(error)))
}

/// The end of a response stream as one status: the server's, or `OK` with the trailers.
async fn final_status<T>(mut streaming: Streaming<T>) -> Status {
    match streaming.trailers().await {
        Ok(trailers) => Status::with_metadata(Code::Ok, "", trailers.unwrap_or_default()),
        Err(status) => status,
    }
}

/// One method call over a connected channel.
pub struct TonicTransport {
    pub channel: Channel,
    pub method: MethodDescriptor,
    pub metadata: MetadataMap,
    /// Sent as `grpc-timeout`; the call task keeps the client timer.
    pub deadline: Option<std::time::Duration>,
}

impl TonicTransport {
    pub fn new(channel: Channel, method: MethodDescriptor, spec: &GrpcCallSpec) -> Self {
        Self {
            channel,
            method,
            metadata: MetadataMap::from_headers(spec.metadata.clone()),
            deadline: spec.deadline,
        }
    }
}

impl Transport for TonicTransport {
    fn open(self: Box<Self>, requests: Requests) -> BoxFuture<'static, Result<Opened, Status>> {
        Box::pin(async move {
            let path = PathAndQuery::try_from(path_of(&self.method))
                .map_err(|_| Status::internal("The method path is not valid."))?;
            let mut grpc = client(self.channel);
            ready(&mut grpc).await?;
            let mut request = Request::new(requests);
            *request.metadata_mut() = self.metadata;
            if let Some(deadline) = self.deadline {
                request.set_timeout(deadline);
            }
            let codec = DynamicCodec::new(self.method.output());
            let response = grpc.streaming(request, path, codec).await?;
            let (headers, streaming, _) = response.into_parts();
            let responses = stream::unfold(Some(streaming), |state| async {
                let mut streaming = state?;
                match streaming.message().await {
                    Ok(Some(message)) => Some((Ok(message), Some(streaming))),
                    Ok(None) => Some((Err(final_status(streaming).await), None)),
                    Err(status) => Some((Err(status), None)),
                }
            });
            Ok(Opened {
                headers,
                responses: responses.boxed(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_connector_error_is_found_below_other_errors() {
        #[derive(Debug, thiserror::Error)]
        #[error("outer")]
        struct Outer(#[source] AppError);
        let inner = AppError::new(ErrorCode::Network, "The TLS handshake failed.");

        let found = connector_error(&Outer(inner.clone()));

        assert_eq!(found, Some(inner));
        let other = std::io::Error::other("x");
        assert_eq!(connector_error(&other), None);
    }

    #[tokio::test]
    async fn a_refused_port_and_a_timeout_are_reported_as_such() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let url = url::Url::parse(&format!("http://127.0.0.1:{port}")).unwrap();

        let refused = connect(&GrpcCallSpec::new(url)).await.unwrap_err();

        assert_eq!(refused.code, ErrorCode::Network);
        assert_eq!(refused.message, "Could not connect to the server.");

        // Accepts TCP but never answers the TLS handshake.
        let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = silent.local_addr().unwrap().port();
        let mut spec =
            GrpcCallSpec::new(url::Url::parse(&format!("https://127.0.0.1:{port}")).unwrap());
        spec.timeout = Some(std::time::Duration::from_millis(100));

        let timed_out = connect(&spec).await.unwrap_err();

        assert_eq!(timed_out.code, ErrorCode::Timeout);
        drop(silent);
    }
}
