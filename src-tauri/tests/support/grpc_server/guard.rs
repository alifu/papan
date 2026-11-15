//! Wraps every service: records the request metadata, checks the Bearer token, and
//! copies `x-echo-*` request metadata to response headers and `x-trailer-*` to trailers.

use std::convert::Infallible;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures_util::future::BoxFuture;
use http_body_util::BodyExt;
use tonic::body::Body;
use tonic::codegen::http::{self, HeaderMap};
use tonic::codegen::Service;
use tonic::server::NamedService;
use tonic::Status;

use super::{ReceivedCall, State};

#[derive(Clone)]
pub(super) struct Guarded<S> {
    pub(super) inner: S,
    pub(super) state: Arc<State>,
}

impl<S: NamedService> NamedService for Guarded<S> {
    const NAME: &'static str = S::NAME;
}

impl<S> Service<http::Request<Body>> for Guarded<S>
where
    S: Service<http::Request<Body>, Response = http::Response<Body>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send,
{
    type Response = http::Response<Body>;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Infallible>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: http::Request<Body>) -> Self::Future {
        self.state.record(ReceivedCall::from_request(&request));
        if !self.state.authorized(request.headers()) {
            let status = Status::unauthenticated("missing or invalid bearer token");
            return Box::pin(async move { Ok(status.into_http()) });
        }
        let headers = prefixed(request.headers(), "x-echo-");
        let trailers = prefixed(request.headers(), "x-trailer-");
        let mut inner = self.inner.clone();
        Box::pin(async move {
            let response = inner.call(request).await?;
            Ok(decorate(response, headers, trailers))
        })
    }
}

fn prefixed(headers: &HeaderMap, prefix: &str) -> HeaderMap {
    headers
        .iter()
        .filter(|(name, _)| name.as_str().starts_with(prefix))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

/// Trailers are added only when the response has a trailers frame (not trailers-only).
fn decorate(
    response: http::Response<Body>,
    headers: HeaderMap,
    trailers: HeaderMap,
) -> http::Response<Body> {
    let (mut parts, body) = response.into_parts();
    parts.headers.extend(headers);
    if trailers.is_empty() {
        return http::Response::from_parts(parts, body);
    }
    let body = body.map_frame(move |mut frame| {
        if let Some(map) = frame.trailers_mut() {
            map.extend(trailers.clone());
        }
        frame
    });
    http::Response::from_parts(parts, Body::new(body))
}
