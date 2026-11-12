use super::*;
use crate::store::types::Auth;

fn location() -> RequestLocation {
    RequestLocation::Scratch {
        id: "s".to_string(),
    }
}

fn record(url: &str, content: &RequestContent, outcome: Outcome) -> HistoryRecord {
    let content = RequestContent {
        url: url.to_string(),
        ..content.clone()
    };
    HistoryRecord::new(&Sent {
        location: &location(),
        name: "Req",
        content: &content,
        sent_at_ms: 1.0,
        outcome,
    })
}

fn ok() -> Outcome {
    Outcome::Response {
        status: 200,
        total_ms: 5,
        size_bytes: 10,
    }
}

#[test]
fn appended_entries_are_listed_newest_first_and_capped() {
    let dir = tempfile::tempdir().unwrap();
    let history = History::new(dir.path());
    let base = RequestContent::new("GET", "");

    for index in 0..=MAX_HISTORY {
        history
            .append(record(&format!("https://x/{index}"), &base, ok()))
            .unwrap();
    }
    let listed = history.list();

    assert_eq!(listed.len(), MAX_HISTORY);
    assert_eq!(listed[0].url, format!("https://x/{MAX_HISTORY}"));
    assert_eq!(listed[MAX_HISTORY - 1].url, "https://x/1");
}

#[test]
fn records_are_redacted_and_drop_big_bodies() {
    let content = RequestContent {
        auth: Auth::Bearer {
            token: "secret".into(),
        },
        body: Body::Text {
            text: "x".repeat(MAX_SNAPSHOT_BODY_BYTES + 1),
        },
        ..RequestContent::new("POST", "")
    };

    let stored = record("https://x", &content, ok());

    assert_eq!(
        stored.content.auth,
        Auth::Bearer {
            token: String::new()
        }
    );
    assert_eq!(
        stored.content.body,
        Body::Text {
            text: String::new()
        }
    );
    assert_eq!(stored.entry.status, Some(200));
}

#[test]
fn failures_store_the_error_code() {
    let stored = record(
        "https://x",
        &RequestContent::new("GET", ""),
        Outcome::Failed {
            code: ErrorCode::Timeout,
            total_ms: 30,
        },
    );

    assert_eq!(
        (
            stored.entry.status,
            stored.entry.error_code,
            stored.entry.total_ms
        ),
        (None, Some(ErrorCode::Timeout), 30)
    );
}

#[test]
fn malformed_file_counts_as_empty_and_clear_empties() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(HISTORY_FILE), "{not json").unwrap();
    let history = History::new(dir.path());

    let before = history.list();
    history
        .append(record("https://x", &RequestContent::new("GET", ""), ok()))
        .unwrap();
    let id = history.list()[0].id.clone();
    let found = history.find(&id).unwrap();
    history.clear().unwrap();

    assert!(before.is_empty());
    assert_eq!(found.location, location());
    assert!(history.list().is_empty());
    assert_eq!(history.find(&id).unwrap_err().code, ErrorCode::NotFound);
}
