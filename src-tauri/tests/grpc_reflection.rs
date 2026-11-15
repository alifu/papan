//! Server reflection through the engine (plan 08 §9): v1, the v1alpha fallback, no
//! reflection, dependencies across files, auth on reflection, the descriptor cache, and a
//! call whose descriptors came from reflection.

mod support;

use papan_lib::engine::grpc::types::GrpcMethodKind;
use papan_lib::error::ErrorCode;
use papan_lib::store::types::{Auth, GrpcSource};
use support::grpc_server::{self, Options, Reflection};
use support::grpc_world::{cid, reflected, world, SECRET};

fn with_reflection(reflection: Reflection) -> Options {
    Options {
        reflection,
        ..Options::default()
    }
}

#[tokio::test]
async fn reflection_v1_lists_the_service_and_follows_its_imports() {
    let (w, server) = (
        world(),
        grpc_server::spawn_with(with_reflection(Reflection::V1Only)).await,
    );

    let schema = w.schema(&reflected(&server.url(), "", "")).await.unwrap();

    assert_eq!(schema.source, GrpcSource::Reflection);
    let names: Vec<&str> = schema
        .services
        .iter()
        .map(|s| s.full_name.as_str())
        .collect();
    assert_eq!(names, ["test.v1.Echo"], "reflection itself is hidden");
    let echo = &schema.services[0];
    let kind = |name: &str| echo.methods.iter().find(|m| m.name == name).unwrap().kind;
    assert_eq!(kind("Unary"), GrpcMethodKind::Unary);
    assert_eq!(kind("ServerStream"), GrpcMethodKind::ServerStreaming);
    assert_eq!(kind("ClientStream"), GrpcMethodKind::ClientStreaming);
    assert_eq!(kind("Bidi"), GrpcMethodKind::Bidi);
    assert!(
        schema.files.contains(&"test/v1/types.proto".to_string()),
        "{:?}",
        schema.files
    );
    assert!(schema.fetched_at_ms.is_some());
    assert!(server
        .calls()
        .iter()
        .all(|c| c.path.contains(".v1.ServerReflection/")));
}

#[tokio::test]
async fn a_v1alpha_only_server_is_reflected_through_the_fallback() {
    let (w, server) = (
        world(),
        grpc_server::spawn_with(with_reflection(Reflection::V1AlphaOnly)).await,
    );

    let schema = w
        .reflect(&reflected(&server.url(), "", ""), true)
        .await
        .unwrap();

    assert_eq!(schema.services[0].full_name, "test.v1.Echo");
    let paths: Vec<String> = server.calls().into_iter().map(|c| c.path).collect();
    assert!(paths.iter().any(|p| p.contains("v1alpha")), "{paths:?}");
}

#[tokio::test]
async fn a_server_without_reflection_gives_a_clear_error() {
    let (w, server) = (
        world(),
        grpc_server::spawn_with(with_reflection(Reflection::Off)).await,
    );

    let error = w
        .reflect(&reflected(&server.url(), "", ""), true)
        .await
        .unwrap_err();

    assert_ne!(error.code, ErrorCode::Internal, "{error:?}");
    assert!(
        error.message.to_lowercase().contains("reflection"),
        "{error:?}"
    );
}

#[tokio::test]
async fn reflection_sends_the_auth_metadata_and_never_shows_the_secret() {
    let server = grpc_server::spawn_with(Options {
        token: Some(SECRET.into()),
        ..Options::default()
    })
    .await;
    let w = world();
    let mut request = reflected(&server.url(), "", "");

    let refused = w.reflect(&request, true).await.unwrap_err();
    request.auth = Auth::Bearer {
        token: "{{token}}".into(),
    };
    let schema = w.reflect(&request, true).await.unwrap();

    assert!(!format!("{refused:?}").contains(SECRET));
    assert_eq!(schema.services[0].full_name, "test.v1.Echo");
    let call = server.calls().into_iter().last().unwrap();
    assert_eq!(
        call.get("authorization"),
        Some(format!("Bearer {SECRET}").as_str())
    );
}

#[tokio::test]
async fn the_cache_answers_without_the_network_until_a_refresh() {
    let (w, server) = (world(), grpc_server::spawn().await);
    let request = reflected(&server.url(), "", "");
    let first = w.reflect(&request, false).await.unwrap();
    let calls = server.calls().len();

    let again = w.schema(&request).await.unwrap();
    assert_eq!(server.calls().len(), calls, "the cache was not used");
    assert_eq!(again.fetched_at_ms, first.fetched_at_ms);
    server.stop().await;
    let offline = w.reflect(&request, false).await.unwrap();
    let refreshed = w.reflect(&request, true).await.unwrap_err();

    assert_eq!(offline.services, first.services);
    assert_eq!(refreshed.code, ErrorCode::Network, "{refreshed:?}");
    let on_disk = std::fs::read_dir(w.dir.path().join("app-data/grpc-descriptors")).unwrap();
    assert_eq!(on_disk.count(), 1);
}

#[tokio::test]
async fn a_call_with_reflected_descriptors_reaches_the_method() {
    let (w, server) = (world(), grpc_server::spawn().await);
    let request = reflected(&server.url(), "Unary", r#"{"message":"via reflection"}"#);

    w.start(&cid(1), &request).await.unwrap();
    let finish = w.finished(&cid(1), 1).await;

    assert_eq!(finish.status.name, "OK");
    assert!(w.received(&cid(1))[0].contains("via reflection"));
    // The descriptors were fetched once and cached for the next start.
    let reflections = |s: &grpc_server::GrpcServer| {
        s.calls()
            .iter()
            .filter(|c| c.path.contains("Reflection"))
            .count()
    };
    let before = reflections(&server);
    w.start(&cid(2), &request).await.unwrap();
    w.finished(&cid(2), 1).await;
    assert_eq!(reflections(&server), before);
}

#[tokio::test]
async fn schema_without_network_sends_nothing_until_there_is_a_cache() {
    let (w, server) = (world(), grpc_server::spawn().await);
    let mut request = reflected(&server.url(), "", "");
    request.auth = Auth::Bearer {
        token: "{{token}}".into(),
    };

    let before = w.schema_offline(&request).await.unwrap();

    assert!(before.is_none());
    assert!(server.calls().is_empty(), "the server was contacted");
    w.reflect(&request, false).await.unwrap();
    let after = w.schema_offline(&request).await.unwrap().unwrap();
    assert_eq!(after.services[0].full_name, "test.v1.Echo");
}
