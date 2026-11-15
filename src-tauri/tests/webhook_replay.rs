//! Replaying captures into scratch requests (plan 07 §10).

mod support;

use papan_lib::store::scratch::Scratch;
use papan_lib::store::types::Body;
use support::webhook::{config, running, url_of, world};

async fn replayed(
    send: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
) -> (papan_lib::store::types::RequestContent, Vec<String>, String) {
    let w = world();
    let l = running(&w, config("/hooks")).await;
    let client = reqwest::Client::new();
    send(client.post(format!("{}/in?x=1", url_of(&l))))
        .send()
        .await
        .unwrap();
    let id = &l.config.id;
    let capture_id = w.service.captures(id).unwrap()[0].id.clone();
    let scratch = Scratch::new(w.dir.path()).unwrap();
    let result = w.service.replay(id, &capture_id, &scratch).unwrap();
    let content = scratch.read(&result.item.id).unwrap().content().unwrap();
    (content, result.warnings, result.item.name)
}

#[tokio::test]
async fn json_capture_becomes_a_json_scratch_request() {
    let (content, warnings, name) = replayed(|r| {
        r.header("Content-Type", "application/json")
            .header("Authorization", "Bearer t")
            .body("{\"a\":1}")
    })
    .await;
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("Authorization/Cookie/signature"));
    assert!(name.starts_with("POST /hooks/in "));
    assert_eq!(content.method, "POST");
    assert!(content.url.starts_with("http://127.0.0.1:"));
    assert!(content.url.ends_with("/hooks/in?x=1"));
    assert!(matches!(content.body, Body::Json { ref text } if text == "{\"a\":1}"));
    let names: Vec<String> = content
        .headers
        .iter()
        .map(|h| h.name.to_lowercase())
        .collect();
    assert!(names.contains(&"authorization".to_string()));
    for dropped in ["host", "content-length", "connection"] {
        assert!(
            !names.contains(&dropped.to_string()),
            "{dropped} should be dropped"
        );
    }
}

#[tokio::test]
async fn form_and_xml_captures_map_to_their_bodies() {
    let (form, _, _) = replayed(|r| {
        r.header("Content-Type", "application/x-www-form-urlencoded")
            .body("a=1&b=x+y")
    })
    .await;
    assert!(
        matches!(form.body, Body::Form { ref fields } if fields.len() == 2 && fields[1].value == "x y")
    );
    let (xml, _, _) = replayed(|r| r.header("Content-Type", "application/xml").body("<a/>")).await;
    assert!(matches!(xml.body, Body::Xml { .. }));
}

#[tokio::test]
async fn binary_capture_warns_and_has_no_body() {
    let (content, warnings, _) = replayed(|r| {
        r.header("Content-Type", "application/octet-stream")
            .body(vec![0xffu8, 0x00, 0xfe])
    })
    .await;
    assert_eq!(content.body, Body::None);
    assert_eq!(warnings.len(), 1);
}

#[tokio::test]
async fn unknown_capture_is_not_found() {
    let w = world();
    let l = running(&w, config("/hooks")).await;
    let scratch = Scratch::new(w.dir.path()).unwrap();
    assert!(w.service.replay(&l.config.id, "nope", &scratch).is_err());
}
