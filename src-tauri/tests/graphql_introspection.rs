//! Schema introspection through the HTTP engine (plan 06 §6, §11).

mod support;

use std::time::Duration;

use papan_lib::engine::graphql::types::SchemaSource;
use papan_lib::error::ErrorCode;
use papan_lib::store::types::{Auth, Body, RequestContent, RequestKind, Scripts};
use support::graphql_server::{self, SECURE_TOKEN};
use support::graphql_world::{new_id, scratch, world};

fn graphql(url: &str, auth: Auth) -> RequestContent {
    RequestContent {
        body: Body::Graphql {
            query: "{ greeting }".into(),
            variables: String::new(),
            operation_name: String::new(),
        },
        kind: RequestKind::Graphql,
        auth,
        ..RequestContent::new("POST", url)
    }
}

fn json_of(source: &SchemaSource) -> serde_json::Value {
    match source {
        SchemaSource::Introspection { json } => serde_json::from_str(json).unwrap(),
        SchemaSource::Sdl { .. } => panic!("expected introspection"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn introspection_returns_the_schema_caches_it_and_writes_no_history() {
    let gql = graphql_server::spawn().await;
    let w = world(&gql.url(""), SECURE_TOKEN);
    let content = RequestContent {
        scripts: Scripts {
            pre_request: "throw new Error('scripts must not run')".into(),
            post_response: String::new(),
        },
        ..graphql("{{base}}/graphql?api_key=k", Auth::None)
    };

    let schema = w
        .graphql()
        .introspect(&new_id(), &scratch(), &content)
        .await
        .unwrap();

    let types = json_of(&schema.source)["data"]["__schema"]["types"].clone();
    let names: Vec<&str> = types
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(
        names.contains(&"User") && names.contains(&"Mutation"),
        "{names:?}"
    );
    assert_eq!(schema.endpoint, gql.url("/graphql"));
    assert_eq!(w.history_len(), 0);
    let cached = w.graphql().read_cached(&scratch(), &content).await.unwrap();
    assert_eq!(cached, Some(schema));
    w.graphql()
        .clear_cached(&scratch(), &content)
        .await
        .unwrap();
    assert_eq!(
        w.graphql().read_cached(&scratch(), &content).await.unwrap(),
        None
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn bearer_auth_from_a_secret_is_sent_and_masked() {
    let gql = graphql_server::spawn().await;
    let w = world(&gql.url(""), SECURE_TOKEN);
    let bearer = Auth::Bearer {
        token: "{{token}}".into(),
    };

    let ok = w
        .graphql()
        .introspect(
            &new_id(),
            &scratch(),
            &graphql("{{base}}/secure/graphql", bearer),
        )
        .await;
    let refused = w
        .graphql()
        .introspect(
            &new_id(),
            &scratch(),
            &graphql("{{base}}/secure/graphql", Auth::None),
        )
        .await
        .unwrap_err();

    assert!(ok.is_ok(), "{ok:?}");
    assert_eq!(
        gql.authorizations()[0].as_deref(),
        Some(format!("Bearer {SECURE_TOKEN}").as_str())
    );
    assert!(refused.message.contains("401"), "{}", refused.message);
    assert!(!format!("{refused:?}").contains(SECURE_TOKEN));
}

#[tokio::test(flavor = "multi_thread")]
async fn introspection_can_be_cancelled() {
    let gql = graphql_server::spawn().await;
    let w = world(&gql.url(""), "t");
    let id = new_id();
    let content = graphql("{{base}}/slow", Auth::None);

    let deps = w.graphql();
    let location = scratch();
    let running = deps.introspect(&id, &location, &content);
    let cancel = async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        w.engine.cancel(&id).unwrap();
    };
    let (result, ()) = tokio::join!(running, cancel);

    assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_schema_file_in_sdl_or_json_is_stored_for_the_endpoint() {
    let w = world("http://api.test", "t");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/graphql");
    let content = graphql("{{base}}/graphql", Auth::None);

    let sdl = std::fs::read(dir.join("schema.graphql")).unwrap();
    let from_sdl = w
        .graphql()
        .store_file(&scratch(), &content, &sdl)
        .await
        .unwrap();
    let json = std::fs::read(dir.join("introspection.json")).unwrap();
    let from_json = w
        .graphql()
        .store_file(&scratch(), &content, &json)
        .await
        .unwrap();

    assert!(matches!(from_sdl.source, SchemaSource::Sdl { .. }));
    assert_eq!(from_json.endpoint, "http://api.test/graphql");
    assert_eq!(
        w.graphql().read_cached(&scratch(), &content).await.unwrap(),
        Some(from_json)
    );
}
