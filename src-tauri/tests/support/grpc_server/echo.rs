//! `test.v1.Echo`, hand-written over `tonic::server::Grpc` + `ProstCodec`.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use futures_util::future::BoxFuture;
use futures_util::{Stream, StreamExt};
use tonic::body::Body;
use tonic::codegen::{http, Service};
use tonic::server::{Grpc, NamedService};
use tonic::{Code, Request, Response, Status, Streaming};
use tonic_prost::ProstCodec;

use super::messages::*;
use super::State;

type MessageStream = Pin<Box<dyn Stream<Item = Result<EchoResponse, Status>> + Send>>;

/// A tower service from a closure (tower is not a direct dependency).
struct Handler<F>(F);

impl<F, Req, Fut, Resp> Service<Req> for Handler<F>
where
    F: FnMut(Req) -> Fut,
    Fut: Future<Output = Result<Resp, Status>>,
{
    type Response = Resp;
    type Error = Status;
    type Future = Fut;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Status>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Req) -> Fut {
        (self.0)(request)
    }
}

#[derive(Clone)]
pub(super) struct Echo {
    pub(super) state: Arc<State>,
}

impl NamedService for Echo {
    const NAME: &'static str = "test.v1.Echo";
}

impl Service<http::Request<Body>> for Echo {
    type Response = http::Response<Body>;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Infallible>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: http::Request<Body>) -> Self::Future {
        let state = Arc::clone(&self.state);
        Box::pin(async move { Ok(route(state, request).await) })
    }
}

fn grpc<E, D>() -> Grpc<ProstCodec<E, D>>
where
    E: prost::Message + Send + 'static,
    D: prost::Message + Default + Send + 'static,
{
    Grpc::new(ProstCodec::default())
}

async fn route(state: Arc<State>, request: http::Request<Body>) -> http::Response<Body> {
    let method = request.uri().path().rsplit('/').next().unwrap_or_default();
    match method {
        "Unary" => grpc().unary(Handler(unary), request).await,
        "ServerStream" => {
            let handler = Handler(move |r| server_stream(Arc::clone(&state), r));
            grpc().server_streaming(handler, request).await
        }
        "ClientStream" => {
            grpc()
                .client_streaming(Handler(client_stream), request)
                .await
        }
        "Bidi" => {
            let handler = Handler(move |r| bidi(Arc::clone(&state), r));
            grpc().streaming(handler, request).await
        }
        "Fail" => grpc().unary(Handler(fail), request).await,
        "Sleep" => {
            let handler = Handler(move |r| sleep(Arc::clone(&state), r));
            grpc().unary(handler, request).await
        }
        "Large" => grpc().unary(Handler(large), request).await,
        _ => Status::unimplemented(format!("unknown method {method}")).into_http(),
    }
}

fn reply(message: impl Into<String>, index: i32) -> EchoResponse {
    EchoResponse {
        message: message.into(),
        index,
    }
}

async fn unary(request: Request<EchoRequest>) -> Result<Response<EchoResponse>, Status> {
    Ok(Response::new(reply(request.into_inner().message, 0)))
}

async fn fail(request: Request<FailRequest>) -> Result<Response<EchoResponse>, Status> {
    let FailRequest {
        code,
        message,
        details,
    } = request.into_inner();
    Err(Status::with_details(
        Code::from(code),
        message,
        details.into(),
    ))
}

async fn large(request: Request<LargeRequest>) -> Result<Response<EchoResponse>, Status> {
    let size = usize::try_from(request.into_inner().size).unwrap_or_default();
    Ok(Response::new(reply("x".repeat(size), 0)))
}

/// Counts a cancellation when dropped before `done`.
struct CancelGuard {
    state: Arc<State>,
    done: bool,
}

impl Drop for CancelGuard {
    fn drop(&mut self) {
        if !self.done {
            self.state.note_cancel();
        }
    }
}

async fn sleep(
    state: Arc<State>,
    request: Request<SleepRequest>,
) -> Result<Response<EchoResponse>, Status> {
    let ms = request.into_inner().ms;
    let mut guard = CancelGuard { state, done: false };
    if ms <= 0 {
        std::future::pending::<()>().await;
    }
    tokio::time::sleep(Duration::from_millis(ms.unsigned_abs().into())).await;
    guard.done = true;
    Ok(Response::new(reply("awake", 0)))
}

/// Feeds a channel from a spawned task; a closed channel means the client went away.
fn channel_stream() -> (
    tokio::sync::mpsc::Sender<Result<EchoResponse, Status>>,
    MessageStream,
) {
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    });
    (tx, Box::pin(stream))
}

async fn server_stream(
    state: Arc<State>,
    request: Request<StreamRequest>,
) -> Result<Response<MessageStream>, Status> {
    let spec = request.into_inner();
    let (tx, stream) = channel_stream();
    tokio::spawn(async move {
        for index in 0..spec.count {
            if spec.fail_at > 0 && index == spec.fail_at {
                let _ = tx.send(Err(Status::aborted("fail_at reached"))).await;
                return;
            }
            if index > 0 && spec.delay_ms > 0 {
                let delay = Duration::from_millis(spec.delay_ms.unsigned_abs().into());
                tokio::time::sleep(delay).await;
            }
            let item = reply(format!("{} {index}", spec.message), index);
            if tx.send(Ok(item)).await.is_err() {
                state.note_cancel();
                return;
            }
        }
    });
    Ok(Response::new(stream))
}

async fn client_stream(
    request: Request<Streaming<SumRequest>>,
) -> Result<Response<SumResponse>, Status> {
    let mut inbound = request.into_inner();
    let mut total = SumResponse::default();
    while let Some(item) = inbound.message().await? {
        total.sum += item.value;
        total.count += 1;
    }
    Ok(Response::new(total))
}

async fn bidi(
    state: Arc<State>,
    request: Request<Streaming<EchoRequest>>,
) -> Result<Response<MessageStream>, Status> {
    let mut inbound = request.into_inner();
    let (tx, stream) = channel_stream();
    tokio::spawn(async move {
        let mut index = 0;
        loop {
            let item = match inbound.next().await {
                Some(Ok(item)) => item,
                Some(Err(_)) => return state.note_cancel(),
                None => return,
            };
            let bye = item.message == "bye";
            if tx.send(Ok(reply(item.message, index))).await.is_err() {
                return state.note_cancel();
            }
            if bye {
                return;
            }
            index += 1;
        }
    });
    Ok(Response::new(stream))
}
