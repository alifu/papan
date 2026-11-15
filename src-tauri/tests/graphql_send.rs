//! GraphQL requests through the unchanged send pipeline (plan 06 §5, §11).

mod support;

use papan_lib::engine::http::types::SendOutcome;
use papan_lib::store::types::{Body, RequestContent, RequestKind, RequestLocation, Scripts};
use serde_json::Value;
use support::graphql_server;
use support::graphql_world::{new_id, world, World};

const TWO_OPERATIONS: &str = "query Greet($name: String) { greeting(name: $name) }\n\
query GetUser($id: ID!) { user(id: $id) { id name } }\n";

fn graphql(query: &str, variables: &str, operation_name: &str, scripts: Scripts) -> RequestContent {
    RequestContent {
        body: Body::Graphql {
            query: query.into(),
            variables: variables.into(),
            operation_name: operation_name.into(),
        },
        kind: RequestKind::Graphql,
        scripts,
        ..RequestContent::new("POST", "{{base}}/graphql")
    }
}

async fn response(w: &World, content: &RequestContent) -> (SendOutcome, Value) {
    let id = new_id();
    let outcome = w.send(&id, content).await.unwrap();
    let body = serde_json::from_str(&w.engine.read_body(&id).unwrap().data).unwrap();
    (outcome, body)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_operation_name_selects_the_operation_and_variables_are_sent() {
    let gql = graphql_server::spawn().await;
    let w = world(&gql.url(""), "t");

    let (greet, greet_body) = response(
        &w,
        &graphql(
            TWO_OPERATIONS,
            r#"{"name": "Ada"}"#,
            "Greet",
            Scripts::default(),
        ),
    )
    .await;
    let (_, user_body) = response(
        &w,
        &graphql(
            TWO_OPERATIONS,
            r#"{"id": "7"}"#,
            "GetUser",
            Scripts::default(),
        ),
    )
    .await;

    assert_eq!(greet.meta.unwrap().status, 200);
    assert_eq!(greet_body["data"]["greeting"], "Hello, Ada!");
    assert_eq!(user_body["data"]["user"]["name"], "User 7");
    assert_eq!(w.history_len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_errors_response_is_a_normal_response() {
    let gql = graphql_server::spawn().await;
    let w = world(&gql.url(""), "t");

    let (outcome, body) = response(&w, &graphql("{ nope }", "", "", Scripts::default())).await;

    assert_eq!(outcome.meta.unwrap().status, 200);
    assert!(body["errors"][0]["message"]
        .as_str()
        .unwrap()
        .contains("nope"));
}

#[tokio::test(flavor = "multi_thread")]
async fn scripts_set_variables_and_test_the_graphql_response() {
    let gql = graphql_server::spawn().await;
    let w = world(&gql.url(""), "t");
    let scripts = Scripts {
        enabled: true,
        pre_request: r#"pm.variables.set("uid", "42");"#.into(),
        post_response: r#"pm.test("user", () => {
  pm.expect(pm.response.json().data.user.id).to.eql("42");
});"#
            .into(),
    };

    let (outcome, body) = response(
        &w,
        &graphql(TWO_OPERATIONS, r#"{"id": "{{uid}}"}"#, "GetUser", scripts),
    )
    .await;

    assert_eq!(body["data"]["user"]["id"], "42");
    let post = outcome.scripts.last().unwrap();
    assert_eq!(post.tests.len(), 1);
    assert!(post.tests[0].passed, "{:?}", post.tests[0]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_script_that_breaks_the_graphql_body_stops_the_send_and_history_keeps_the_draft() {
    let gql = graphql_server::spawn().await;
    let w = world(&gql.url(""), "t");
    let scripts = Scripts {
        enabled: true,
        pre_request: r#"pm.request.body = "[1]";"#.into(),
        post_response: String::new(),
    };
    let draft = graphql("{ greeting }", "", "", scripts);

    let outcome = w.send(&new_id(), &draft).await;

    let error = match outcome {
        Ok(outcome) => outcome.error.expect("the send must fail"),
        Err(error) => error,
    };
    assert!(error.message.contains("GraphQL"), "{}", error.message);
    assert_eq!(gql.hits(), 0);
    let entries = w.store.lock_store().list_history();
    assert_eq!(entries.len(), 1);
    let restored = w.store.lock_store().restore_history_entry(&entries[0].id);
    let location = RequestLocation::Scratch {
        id: restored.unwrap().id,
    };
    let doc = w.store.lock_store().read_request(&location).unwrap();
    assert_eq!(doc.content.body, draft.body);
    assert_eq!(doc.content.kind, RequestKind::Graphql);
}
