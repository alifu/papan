//! Streaming gRPC calls through the engine against the test server (plan 08 §9): server,
//! client and bidi streams, cancel mid-stream, the log ring, batching and event order,
//! and restarts on the same id.

mod support;

use std::time::Duration;

use papan_lib::engine::grpc::types::{GrpcEvent, GrpcEventKind};
use papan_lib::engine::ws::log::MAX_ENTRIES;
use papan_lib::engine::ws::types::WsDirection;
use papan_lib::error::ErrorCode;
use support::grpc_server;
use support::grpc_world::{cid, content, messages_of, world};

const WAIT: Duration = Duration::from_secs(5);

fn stream(message: &str, count: i32, delay_ms: i32, fail_at: i32) -> String {
    format!(r#"{{"message":"{message}","count":{count},"delayMs":{delay_ms},"failAt":{fail_at}}}"#)
}

fn index_of(text: &str) -> i64 {
    let value: serde_json::Value = serde_json::from_str(text).unwrap();
    value["index"].as_i64().unwrap_or_default()
}

/// Exactly one `finished`, and nothing after it.
fn assert_finished_last(events: &[GrpcEvent]) {
    let finished: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e.kind, GrpcEventKind::Finished { .. }))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(finished, [events.len() - 1], "{events:?}");
}

#[tokio::test]
async fn a_server_stream_arrives_in_order_and_fail_at_ends_it_after_partial_messages() {
    let (w, server) = (world(), grpc_server::spawn().await);
    let url = server.url();

    w.start(
        &cid(1),
        &content(&url, "ServerStream", &stream("m", 5, 5, 0)),
    )
    .await
    .unwrap();
    let ok = w.finished(&cid(1), 1).await;
    w.start(
        &cid(2),
        &content(&url, "ServerStream", &stream("m", 5, 0, 2)),
    )
    .await
    .unwrap();
    let failed = w.finished(&cid(2), 1).await;

    assert_eq!(ok.status.name, "OK");
    let indexes: Vec<i64> = w.received(&cid(1)).iter().map(|t| index_of(t)).collect();
    assert_eq!(indexes, [0, 1, 2, 3, 4]);
    assert_finished_last(&w.events_of(&cid(1)));
    assert_eq!(failed.status.name, "ABORTED");
    assert_eq!(w.received(&cid(2)).len(), 2);
    let log = w.state.engine.log(&cid(1));
    assert_eq!(log[0].direction, WsDirection::Sent);
    assert!(log.windows(2).all(|pair| pair[0].at_ms <= pair[1].at_ms));
}

#[tokio::test]
async fn a_client_stream_sums_what_was_sent_and_ending_twice_is_fine() {
    let (w, server) = (world(), grpc_server::spawn().await);
    let id = cid(1);

    w.start(&id, &content(&server.url(), "ClientStream", ""))
        .await
        .unwrap();
    for value in [1, 2, 39] {
        w.send(&id, &format!(r#"{{"value":"{value}"}}"#))
            .await
            .unwrap();
    }
    w.state.engine.end(&id).unwrap();
    w.state.engine.end(&id).unwrap();
    let after_end = w.send(&id, r#"{"value":"1"}"#).await.unwrap_err();
    let finish = w.finished(&id, 1).await;

    assert_eq!(finish.status.name, "OK");
    let reply: serde_json::Value = serde_json::from_str(&w.received(&id)[0]).unwrap();
    assert_eq!(reply["sum"], "42");
    assert_eq!(reply["count"], 3);
    assert_eq!(after_end.code, ErrorCode::InvalidInput);
    // Messages sent later are returned by `send` (and logged), not emitted again.
    assert!(messages_of(&w.events_of(&id), WsDirection::Sent).is_empty());
    let log = w.state.engine.log(&id);
    let sent = log.iter().filter(|m| m.direction == WsDirection::Sent);
    assert_eq!(sent.count(), 3, "{log:?}");
}

#[tokio::test]
async fn bidi_interleaves_and_the_server_closes_on_bye() {
    let (w, server) = (world(), grpc_server::spawn().await);
    let id = cid(1);

    w.start(&id, &content(&server.url(), "Bidi", ""))
        .await
        .unwrap();
    w.send(&id, r#"{"message":"a"}"#).await.unwrap();
    w.until_received(&id, 1).await;
    w.send(&id, r#"{"message":"bye"}"#).await.unwrap();
    let finish = w.finished(&id, 1).await;
    let late = w.send(&id, r#"{"message":"late"}"#).await.unwrap_err();

    assert_eq!(
        (finish.status.name.as_str(), finish.by.as_str()),
        ("OK", "Server")
    );
    let directions: Vec<WsDirection> = w
        .state
        .engine
        .log(&id)
        .iter()
        .map(|m| m.direction)
        .collect();
    use WsDirection::{Received, Sent};
    assert_eq!(directions, [Sent, Received, Sent, Received]);
    assert_eq!(late.message, "The call has ended.");
    assert_finished_last(&w.events_of(&id));
}

#[tokio::test]
async fn a_unary_call_takes_no_further_message() {
    let (w, server) = (world(), grpc_server::spawn().await);
    let id = cid(1);

    w.start(&id, &content(&server.url(), "Sleep", r#"{"ms":0}"#))
        .await
        .unwrap();
    let refused = w.send(&id, r#"{"ms":1}"#).await.unwrap_err();
    w.state.engine.cancel(&id);
    let finish = w.finished(&id, 1).await;

    assert!(refused.message.contains("takes one message"), "{refused:?}");
    assert_eq!(
        (finish.status.name.as_str(), finish.by.as_str()),
        ("CANCELLED", "Client")
    );
    assert!(server.wait_for_cancels(1, WAIT).await);
}

#[tokio::test]
async fn cancel_mid_stream_is_seen_by_the_server() {
    let (w, server) = (world(), grpc_server::spawn().await);
    let id = cid(1);

    w.start(
        &id,
        &content(&server.url(), "ServerStream", &stream("m", 1000, 20, 0)),
    )
    .await
    .unwrap();
    w.until_received(&id, 1).await;
    w.state.engine.cancel(&id);
    let finish = w.finished(&id, 1).await;

    assert_eq!(finish.status.name, "CANCELLED");
    assert!(server.wait_for_cancels(1, WAIT).await);
    assert_finished_last(&w.events_of(&id));
    w.idle().await;
}

#[tokio::test]
async fn the_log_keeps_the_latest_entries_and_events_come_in_batches() {
    let (w, server) = (world(), grpc_server::spawn().await);
    let id = cid(1);
    let count = MAX_ENTRIES as i32 + 1;

    w.start(
        &id,
        &content(&server.url(), "ServerStream", &stream("m", count, 0, 0)),
    )
    .await
    .unwrap();
    w.finished(&id, 1).await;

    let log = w.state.engine.log(&id);
    assert_eq!(log.len(), MAX_ENTRIES);
    assert_eq!(index_of(&log.last().unwrap().text), i64::from(count - 1));
    assert_eq!(
        log[0].direction,
        WsDirection::Received,
        "the sent request was dropped first"
    );
    let events = w.events_of(&id);
    let batches = events
        .iter()
        .filter(|e| matches!(e.kind, GrpcEventKind::Messages { .. }))
        .count();
    assert!(
        batches < 100,
        "{batches} message events for {count} messages"
    );
    assert_eq!(w.received(&id).len(), count as usize);
    assert_finished_last(&events);
    w.state.engine.clear_log(&id);
    assert!(w.state.engine.log(&id).is_empty());
}

#[tokio::test]
async fn a_restart_on_the_same_id_finishes_the_old_attempt_before_the_new_one_speaks() {
    let (w, server) = (world(), grpc_server::spawn().await);
    let (id, url) = (cid(1), server.url());
    let long = content(&url, "ServerStream", &stream("old", 1000, 10, 0));

    w.start(&id, &long).await.unwrap();
    w.until_received(&id, 1).await;
    let second = w
        .start(&id, &content(&url, "Unary", r#"{"message":"new"}"#))
        .await
        .unwrap();
    let old = w.finished(&id, 1).await;
    w.finished(&id, 2).await;

    assert_eq!(second.attempt, 2);
    assert_eq!(old.status.name, "CANCELLED");
    let events = w.events_of(&id);
    let first_new = events.iter().position(|e| e.attempt == 2).unwrap();
    let last_old = events.iter().rposition(|e| e.attempt == 1).unwrap();
    assert!(last_old < first_new, "attempts interleaved: {events:?}");
    let new_log = w.state.engine.log(&id);
    assert!(
        new_log.iter().all(|m| !m.text.contains("old")),
        "{new_log:?}"
    );
}
