//! Self-test of `support/grpc_server.rs` with a plain tonic client (plan 08 §8 O1).

mod support;

use std::time::Duration;

use futures_util::StreamExt;
use tokio::net::TcpStream;
use tonic::{Code, Request};
use tonic_prost::ProstCodec;

use support::grpc_server::client::{echo, h2c, list_v1, list_v1alpha, path, tls_channel, unary};
use support::grpc_server::messages::*;
use support::grpc_server::{self, ConnectionInfo, Options, Reflection};
use support::mtls::TestCa;

const WAIT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn h2c_unary_echoes_and_records_metadata() {
    let server = grpc_server::spawn().await;
    let channel = h2c(&server).await;
    let mut request = Request::new(EchoRequest {
        message: "hi".into(),
    });
    request
        .metadata_mut()
        .insert("x-echo-name", "papan".parse().unwrap());
    request.metadata_mut().insert_bin(
        "x-blob-bin",
        tonic::metadata::MetadataValue::from_bytes(b"\x00\x01"),
    );
    let mut grpc = tonic::client::Grpc::new(channel);
    grpc.ready().await.unwrap();
    let response = grpc
        .unary(
            request,
            path("Unary"),
            ProstCodec::<EchoRequest, EchoResponse>::default(),
        )
        .await
        .unwrap();

    assert_eq!(response.get_ref().message, "hi");
    assert_eq!(response.metadata().get("x-echo-name").unwrap(), "papan");
    let call = server.last_call("/Unary").unwrap();
    assert_eq!(call.path, "/test.v1.Echo/Unary");
    assert_eq!(call.get("x-echo-name"), Some("papan"));
    assert_eq!(call.get("x-blob-bin"), Some("AAE"));
    assert_eq!(
        server.connections()[0],
        ConnectionInfo {
            alpn: None,
            client_cert: false
        }
    );
}

#[tokio::test]
async fn server_stream_sends_count_and_fails_at() {
    let server = grpc_server::spawn().await;
    let channel = h2c(&server).await;
    let mut grpc = tonic::client::Grpc::new(channel);
    let codec = || ProstCodec::<StreamRequest, EchoResponse>::default();
    let spec = |fail_at| StreamRequest {
        message: "m".into(),
        count: 3,
        delay_ms: 5,
        fail_at,
    };

    grpc.ready().await.unwrap();
    let mut request = Request::new(spec(0));
    request
        .metadata_mut()
        .insert("x-trailer-end", "yes".parse().unwrap());
    let mut stream = grpc
        .server_streaming(request, path("ServerStream"), codec())
        .await
        .unwrap()
        .into_inner();
    let mut messages = Vec::new();
    while let Some(item) = stream.message().await.unwrap() {
        messages.push(item.message);
    }
    assert_eq!(messages, ["m 0", "m 1", "m 2"]);
    assert_eq!(
        stream
            .trailers()
            .await
            .unwrap()
            .unwrap()
            .get("x-trailer-end")
            .unwrap(),
        "yes"
    );

    grpc.ready().await.unwrap();
    let request = Request::new(spec(2));
    let mut stream = grpc
        .server_streaming(request, path("ServerStream"), codec())
        .await
        .unwrap()
        .into_inner();
    assert_eq!(stream.message().await.unwrap().unwrap().index, 0);
    assert_eq!(stream.message().await.unwrap().unwrap().index, 1);
    assert_eq!(stream.message().await.unwrap_err().code(), Code::Aborted);
}

#[tokio::test]
async fn client_stream_sums_and_bidi_closes_on_bye() {
    let server = grpc_server::spawn().await;
    let mut grpc = tonic::client::Grpc::new(h2c(&server).await);

    grpc.ready().await.unwrap();
    let values = futures_util::stream::iter([1, 2, 39].map(|value| SumRequest { value }));
    let codec = ProstCodec::<SumRequest, SumResponse>::default();
    let total = grpc
        .client_streaming(Request::new(values), path("ClientStream"), codec)
        .await
        .unwrap();
    assert_eq!(*total.get_ref(), SumResponse { sum: 42, count: 3 });

    grpc.ready().await.unwrap();
    let words = ["a", "bye", "never"].map(|w| EchoRequest { message: w.into() });
    let codec = ProstCodec::<EchoRequest, EchoResponse>::default();
    let request = Request::new(futures_util::stream::iter(words));
    let stream = grpc
        .streaming(request, path("Bidi"), codec)
        .await
        .unwrap()
        .into_inner();
    let replies: Vec<String> = stream.map(|item| item.unwrap().message).collect().await;
    assert_eq!(replies, ["a", "bye"]);
}

#[tokio::test]
async fn fail_returns_status_with_details_and_large_is_sized() {
    let server = grpc_server::spawn().await;
    let channel = h2c(&server).await;
    let request = Request::new(FailRequest {
        code: 5,
        message: "nope".into(),
        details: vec![1, 2, 3],
    });

    let status = unary::<_, EchoResponse>(&channel, request, "Fail")
        .await
        .unwrap_err();

    assert_eq!(status.code(), Code::NotFound);
    assert_eq!(status.message(), "nope");
    assert_eq!(status.details(), [1, 2, 3]);
    let large: EchoResponse = unary(
        &channel,
        Request::new(LargeRequest { size: 1 << 20 }),
        "Large",
    )
    .await
    .unwrap();
    assert_eq!(large.message.len(), 1 << 20);
}

#[tokio::test]
async fn hanging_sleep_is_cancelled_by_deadline() {
    let server = grpc_server::spawn().await;
    let channel = h2c(&server).await;
    let mut request = Request::new(SleepRequest { ms: 0 });
    request.set_timeout(Duration::from_millis(100));

    let status = unary::<_, EchoResponse>(&channel, request, "Sleep")
        .await
        .unwrap_err();

    assert!(
        matches!(status.code(), Code::DeadlineExceeded | Code::Cancelled),
        "{status:?}"
    );
    assert!(server
        .last_call("/Sleep")
        .unwrap()
        .get("grpc-timeout")
        .is_some());
    assert!(server.wait_for_cancels(1, WAIT).await);
    let quick: EchoResponse = unary(&channel, Request::new(SleepRequest { ms: 5 }), "Sleep")
        .await
        .unwrap();
    assert_eq!(quick.message, "awake");
}

#[tokio::test]
async fn token_is_required_on_every_service() {
    let server = grpc_server::spawn_with(Options {
        token: Some("s3cret".into()),
        ..Options::default()
    })
    .await;
    let channel = h2c(&server).await;

    assert_eq!(
        echo(&channel, "x").await.unwrap_err().code(),
        Code::Unauthenticated
    );
    assert_eq!(
        list_v1(&channel).await.unwrap_err().code(),
        Code::Unauthenticated
    );
    let mut request = Request::new(EchoRequest {
        message: "ok".into(),
    });
    request
        .metadata_mut()
        .insert("authorization", "Bearer s3cret".parse().unwrap());
    let reply: EchoResponse = unary(&channel, request, "Unary").await.unwrap();
    assert_eq!(reply.message, "ok");
}

#[tokio::test]
async fn reflection_serves_v1_and_v1alpha_as_configured() {
    let both = grpc_server::spawn().await;
    let channel = h2c(&both).await;
    let services = list_v1(&channel).await.unwrap();
    assert!(
        services.contains(&"test.v1.Echo".to_string()),
        "{services:?}"
    );
    assert!(list_v1alpha(&channel)
        .await
        .unwrap()
        .contains(&"test.v1.Echo".to_string()));

    let alpha_only = grpc_server::spawn_with(Options {
        reflection: Reflection::V1AlphaOnly,
        ..Options::default()
    })
    .await;
    let channel = h2c(&alpha_only).await;
    assert_eq!(
        list_v1(&channel).await.unwrap_err().code(),
        Code::Unimplemented
    );
    assert!(list_v1alpha(&channel).await.is_ok());
    let names: Vec<_> = grpc_server::descriptor_set()
        .file
        .into_iter()
        .filter_map(|f| f.name)
        .collect();
    assert_eq!(names, ["test/v1/types.proto", "test/v1/echo.proto"]);
}

#[tokio::test]
async fn tls_negotiates_h2() {
    let server = grpc_server::spawn_tls().await;
    assert!(server.url().starts_with("https://127.0.0.1:"));
    let pem = server.cert_pem.clone().unwrap();
    let channel = tls_channel(&server, &pem, None).await.unwrap();

    assert_eq!(echo(&channel, "secure").await.unwrap().message, "secure");
    let info = &server.connections()[0];
    assert_eq!(info.alpn.as_deref(), Some(&b"h2"[..]));
    assert!(!info.client_cert);
}

#[tokio::test]
async fn mtls_requires_a_client_certificate() {
    let ca = TestCa::new();
    let server = grpc_server::spawn_with(Options {
        tls: Some(ca.server_config()),
        ..Options::default()
    })
    .await;
    let client = ca.client("papan-client");

    let with_cert = tls_channel(&server, &ca.ca_pem(), Some(&client))
        .await
        .unwrap();
    assert_eq!(echo(&with_cert, "mutual").await.unwrap().message, "mutual");
    assert!(server.connections().iter().any(|c| c.client_cert));

    // TLS 1.3 reports the missing certificate after the client handshake, on first use.
    let failed = match tls_channel(&server, &ca.ca_pem(), None).await {
        Err(_) => true,
        Ok(channel) => echo(&channel, "x").await.is_err(),
    };
    assert!(failed);
}

#[tokio::test]
async fn stop_closes_the_port() {
    let server = grpc_server::spawn().await;
    let addr = server.addr;
    server.stop().await;

    assert!(TcpStream::connect(addr).await.is_err());
}
