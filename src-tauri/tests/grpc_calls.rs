//! Unary gRPC calls through the engine against the test server (plan 08 §9): variables and
//! secrets, metadata and `-bin`, Bearer, statuses with details, trailers, deadlines, the
//! size cap, the call limit, and that no secret reaches the log, events or errors.

mod support;

use std::time::Duration;

use papan_lib::engine::grpc::MAX_CALLS;
use papan_lib::engine::ws::types::WsDirection;
use papan_lib::error::ErrorCode;
use papan_lib::store::types::Auth;
use support::grpc_server::{self, Options};
use support::grpc_world::{cid, content, row, world, SECRET};

const WAIT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn a_unary_call_logs_its_first_message_and_masks_the_secret_it_used() {
    let (w, server) = (world(), grpc_server::spawn().await);
    let id = cid(1);
    let request = content(&server.url(), "Unary", r#"{"message":"hi {{token}}"}"#);

    let started = w.start(&id, &request).await.unwrap();
    let finish = w.finished(&id, 1).await;

    assert_eq!(
        (started.attempt, started.path.as_str()),
        (1, "/test.v1.Echo/Unary")
    );
    assert_eq!(finish.status.name, "OK");
    assert_eq!(finish.by, "Server");
    let log = w.state.engine.log(&id);
    assert_eq!(log.len(), 2, "{log:?}");
    assert_eq!(log[0].direction, WsDirection::Sent);
    assert!(log[0].text.contains("hi "), "{}", log[0].text);
    assert_eq!(log[1].direction, WsDirection::Received);
    let shown = format!("{log:?} {:?} {started:?}", w.events_of(&id));
    assert!(!shown.contains(SECRET), "{shown}");
    // The reply echoed the secret, so the received entry was masked too.
    assert!(server.last_call("/Unary").is_some());
}

#[tokio::test]
async fn metadata_and_bin_values_round_trip_and_come_back_as_headers_and_trailers() {
    let (w, server) = (world(), grpc_server::spawn().await);
    let id = cid(1);
    let mut request = content(&server.url(), "Unary", r#"{"message":"m"}"#);
    request.headers = vec![
        row("x-echo-who", "papan"),
        row("x-blob-bin", "AAE="),
        row("x-trailer-end", "done"),
    ];

    w.start(&id, &request).await.unwrap();
    let finish = w.finished(&id, 1).await;

    let call = server.last_call("/Unary").unwrap();
    assert_eq!(call.get("x-echo-who"), Some("papan"));
    assert_eq!(call.get("x-blob-bin"), Some("AAE"));
    assert!(call.get("user-agent").unwrap().contains("papan/9.9.9"));
    let headers = w
        .events_of(&id)
        .into_iter()
        .find_map(|event| match event.kind {
            papan_lib::engine::grpc::types::GrpcEventKind::Headers { metadata } => Some(metadata),
            _ => None,
        });
    let headers = headers.expect("a headers event");
    assert!(headers
        .iter()
        .any(|r| r.name == "x-echo-who" && r.value == "papan"));
    assert_eq!(finish.trailer("x-trailer-end"), Some("done"));
}

#[tokio::test]
async fn bearer_auth_reaches_the_server_and_a_wrong_token_is_unauthenticated() {
    let server = grpc_server::spawn_with(Options {
        token: Some(SECRET.into()),
        ..Options::default()
    })
    .await;
    let w = world();
    let mut request = content(&server.url(), "Unary", r#"{"message":"m"}"#);
    request.auth = Auth::Bearer {
        token: "{{token}}".into(),
    };

    w.start(&cid(1), &request).await.unwrap();
    let ok = w.finished(&cid(1), 1).await;
    request.auth = Auth::Bearer {
        token: "wrong".into(),
    };
    w.start(&cid(2), &request).await.unwrap();
    let refused = w.finished(&cid(2), 1).await;

    assert_eq!(ok.status.name, "OK");
    let call = server
        .calls()
        .into_iter()
        .find(|c| c.path.ends_with("/Unary"));
    assert_eq!(
        call.unwrap().get("authorization"),
        Some(format!("Bearer {SECRET}").as_str())
    );
    assert_eq!(refused.status.name, "UNAUTHENTICATED");
    assert_eq!(refused.status.message, "missing or invalid bearer token");
    let shown = format!("{:?}", w.events_of(&cid(1)));
    assert!(!shown.contains(SECRET), "{shown}");
}

#[tokio::test]
async fn a_status_with_details_is_reported_and_its_message_is_masked() {
    let (w, server) = (world(), grpc_server::spawn().await);
    let message = r#"{"code":5,"message":"no {{token}} here","details":"AQID"}"#;

    w.start(&cid(1), &content(&server.url(), "Fail", message))
        .await
        .unwrap();
    let finish = w.finished(&cid(1), 1).await;

    assert_eq!(
        (finish.status.code, finish.status.name.as_str()),
        (5, "NOT_FOUND")
    );
    assert_eq!(finish.status.details_base64.as_deref(), Some("AQID"));
    assert!(
        finish.status.message.starts_with("no "),
        "{}",
        finish.status.message
    );
    assert!(
        !finish.status.message.contains(SECRET),
        "{}",
        finish.status.message
    );
    assert_eq!(finish.by, "Server");
    assert!(finish.error.is_none());
}

#[tokio::test]
async fn the_deadline_ends_a_hanging_call_and_is_sent_as_grpc_timeout() {
    let (w, server) = (world(), grpc_server::spawn().await);
    let mut request = content(&server.url(), "Sleep", r#"{"ms":0}"#);
    request.grpc.deadline_ms = 1500;

    w.start(&cid(1), &request).await.unwrap();
    let finish = w.finished(&cid(1), 1).await;

    assert_eq!(finish.status.name, "DEADLINE_EXCEEDED");
    assert_eq!(finish.by, "Client");
    let timeout = server.last_call("/Sleep").unwrap();
    assert!(timeout.get("grpc-timeout").is_some(), "{timeout:?}");
    assert!(server.wait_for_cancels(1, WAIT).await);
}

#[tokio::test]
async fn a_response_over_4_mib_is_resource_exhausted() {
    let (w, server) = (world(), grpc_server::spawn().await);
    let size = 4 * 1024 * 1024 + 1;

    let message = format!(r#"{{"size":{size}}}"#);
    w.start(&cid(1), &content(&server.url(), "Large", &message))
        .await
        .unwrap();
    let finish = w.finished(&cid(1), 1).await;

    assert_eq!(finish.status.name, "RESOURCE_EXHAUSTED", "{finish:?}");
    assert!(w.received(&cid(1)).is_empty());
}

#[tokio::test]
async fn the_call_after_the_limit_is_refused_until_one_ends() {
    let (w, server) = (world(), grpc_server::spawn().await);
    let hang = content(&server.url(), "Sleep", r#"{"ms":0}"#);
    for n in 0..MAX_CALLS {
        w.start(&cid(n as u32), &hang).await.unwrap();
    }

    let refused = w.start(&cid(99), &hang).await.unwrap_err();
    w.state.engine.cancel(&cid(0));
    w.finished(&cid(0), 1).await;
    let after = w.start(&cid(99), &hang).await;

    assert_eq!(refused.code, ErrorCode::InvalidInput, "{refused:?}");
    assert!(after.is_ok(), "{after:?}");
    for n in 1..MAX_CALLS {
        w.state.engine.cancel(&cid(n as u32));
    }
    w.state.engine.cancel(&cid(99));
    w.idle().await;
}

#[tokio::test]
async fn a_bad_message_or_method_fails_the_start_without_leaking_the_secret() {
    let (w, server) = (world(), grpc_server::spawn().await);

    let bad_json = content(&server.url(), "Unary", r#"{"nope":"{{token}}"}"#);
    let error = w.start(&cid(1), &bad_json).await.unwrap_err();
    let unknown = w
        .start(&cid(2), &content(&server.url(), "Missing", "{}"))
        .await
        .unwrap_err();
    let not_uuid = w
        .start("not-a-uuid", &content(&server.url(), "Unary", "{}"))
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(!format!("{error:?}").contains(SECRET), "{error:?}");
    assert!(unknown.message.contains("Missing"), "{unknown:?}");
    assert_eq!(not_uuid.message, "That call id is not valid.");
    assert_eq!(w.state.engine.live_count(), 0);
    // Unknown ids are not errors for cancel and log.
    w.state.engine.cancel(&cid(42));
    assert!(w.state.engine.log(&cid(42)).is_empty());
}
