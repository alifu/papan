use std::collections::BTreeMap;

use super::*;
use crate::engine::http::types::{Header, Timing};

fn kv(name: &str, value: &str, enabled: bool) -> KeyValue {
    KeyValue {
        name: name.into(),
        value: value.into(),
        enabled,
    }
}

fn layer(source: VarSource, vars: &[(&str, Value)]) -> Layer {
    Layer {
        source,
        vars: vars
            .iter()
            .map(|(name, value)| (name.to_string(), value.clone()))
            .collect::<BTreeMap<_, _>>(),
    }
}

#[test]
fn folders_are_listed_outer_to_inner() {
    assert_eq!(folders_of("a/b/r.http.yaml"), ["a", "a/b"]);
    assert!(folders_of("r.http.yaml").is_empty());
}

#[test]
fn an_unchanged_request_keeps_the_draft_exactly() {
    let draft = RequestContent {
        headers: vec![kv("A", "1", true), kv("Off", "x", false)],
        body: Body::Multipart { parts: vec![] },
        ..RequestContent::new("POST", "{{base}}/x")
    };

    let request = script_request(&draft);

    assert_eq!(request.headers, [("A".to_string(), "1".to_string())]);
    assert_eq!(request.body, None);
    assert_eq!(content_after(&draft, &request), draft);
}

#[test]
fn script_changes_make_a_new_draft_with_the_original_body_kind() {
    let draft = RequestContent {
        headers: vec![kv("A", "1", true)],
        body: Body::Json { text: "{}".into() },
        ..RequestContent::new("GET", "http://x")
    };
    let request = ScriptRequest {
        method: "POST".into(),
        url: "http://y".into(),
        headers: vec![("A".into(), "1".into()), ("B".into(), "2".into())],
        body: Some("{\"a\":1}".into()),
    };

    let after = content_after(&draft, &request);

    assert_eq!(
        (after.method.as_str(), after.url.as_str()),
        ("POST", "http://y")
    );
    assert_eq!(after.headers, [kv("A", "1", true), kv("B", "2", true)]);
    assert_eq!(
        after.body,
        Body::Json {
            text: "{\"a\":1}".into()
        }
    );
    assert_eq!(draft.method, "GET");
}

#[test]
fn a_form_body_round_trips_through_its_url_encoded_text() {
    let draft = RequestContent {
        body: Body::Form {
            fields: vec![kv("a b", "1&2", true), kv("off", "x", false)],
        },
        ..RequestContent::new("POST", "http://x")
    };
    let request = script_request(&draft);
    assert_eq!(request.body.as_deref(), Some("a+b=1%262"));

    let changed = ScriptRequest {
        body: Some("a+b=1%262&c=3".into()),
        ..request
    };
    let Body::Form { fields } = content_after(&draft, &changed).body else {
        panic!("form expected")
    };
    assert_eq!(fields, [kv("a b", "1&2", true), kv("c", "3", true)]);
}

#[test]
fn a_removed_body_is_none_and_a_new_one_on_multipart_is_text() {
    let draft = RequestContent {
        body: Body::Text { text: "t".into() },
        ..RequestContent::new("POST", "http://x")
    };
    let cleared = ScriptRequest {
        body: None,
        ..script_request(&draft)
    };
    assert_eq!(content_after(&draft, &cleared).body, Body::None);
    assert_eq!(
        changed_body(&Body::Multipart { parts: vec![] }, Some("x")),
        Body::Text { text: "x".into() }
    );
}

#[test]
fn layers_hold_values_and_secret_names_and_no_environment_when_unselected() {
    let scopes = Scopes::new(vec![
        layer(VarSource::Global, &[("g", Value::plain("1"))]),
        layer(
            VarSource::Collection,
            &[
                ("k", Value::secret(Some("s".into()))),
                ("n", Value::secret(None)),
            ],
        ),
    ]);

    let layers = layers_of(&scopes);

    assert_eq!(layers.globals.values["g"], "1");
    assert_eq!(layers.collection.values.len(), 1);
    assert!(layers.collection.secrets.contains("k") && layers.collection.secrets.contains("n"));
    assert!(layers.environment.is_none());
}

#[test]
fn scopes_after_apply_changes_and_put_runtime_on_top() {
    let scopes = Scopes::new(vec![layer(
        VarSource::Global,
        &[
            ("a", Value::plain("1")),
            ("b", Value::plain("2")),
            ("k", Value::secret(Some("s".into()))),
        ],
    )]);
    let mut context = ScriptContext::new(layers_of(&scopes), ScriptRequest::default());
    context.set(Scope::Globals, "a", "changed").unwrap();
    context.set(Scope::Globals, "k", "s2").unwrap();
    context.unset(Scope::Globals, "b").unwrap();
    context.set(Scope::Runtime, "a", "runtime").unwrap();

    let after = scopes_after(&scopes, &context);

    assert_eq!(after.lookup_with_source("a").unwrap().1, VarSource::Runtime);
    assert_eq!(after.layers[0].vars["a"], Value::plain("changed"));
    assert_eq!(after.layers[0].vars["k"], Value::secret(Some("s2".into())));
    assert_eq!(after.lookup("b"), None);
}

#[test]
fn writes_skip_runtime_and_report_changes_with_nowhere_to_go() {
    let layers = Layers {
        environment: None,
        ..Layers::default()
    };
    let mut context = ScriptContext::new(layers, ScriptRequest::default());
    context.set(Scope::Globals, "g", "1").unwrap();
    context.set(Scope::Collection, "c", "2").unwrap();
    context.set(Scope::Runtime, "r", "3").unwrap();
    let scratch = RequestLocation::Scratch { id: "x".into() };

    let (writes, dropped) = writes_of(&context, &scratch, None);

    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].scope, VarScope::Global);
    assert_eq!(dropped.len(), 1);
    assert!(dropped[0].contains("`c`"));

    let located = RequestLocation::Collection {
        collection_id: "id".into(),
        path: "r.http.yaml".into(),
    };
    let (writes, dropped) = writes_of(&context, &located, Some("dev"));
    assert_eq!(writes.len(), 2);
    assert!(dropped.is_empty());
}

#[test]
fn a_send_request_has_no_auth_and_a_text_body() {
    let base = RequestContent {
        auth: crate::store::types::Auth::Bearer { token: "t".into() },
        ..RequestContent::new("GET", "http://x")
    };
    let request = ScriptRequest {
        method: "PUT".into(),
        url: "http://y".into(),
        headers: vec![("A".into(), "1".into())],
        body: Some("b".into()),
    };

    let content = content_of(&request, &base);

    assert_eq!(content.auth, crate::store::types::Auth::None);
    assert_eq!(content.body, Body::Text { text: "b".into() });
    assert_eq!(content.settings, base.settings);
}

#[test]
fn the_response_maps_status_headers_and_lossy_body() {
    let meta = ResponseMeta {
        request_id: "id".into(),
        status: 201,
        status_text: "Created".into(),
        http_version: "HTTP/1.1".into(),
        url: "http://x".into(),
        headers: vec![Header {
            name: "a".into(),
            value: "1".into(),
        }],
        cookies: vec![],
        content_type: None,
        size_bytes: 2,
        truncated: false,
        ttfb_ms: 1,
        total_ms: 7,
        timing: Timing::default(),
        tls_verified: false,
        unresolved: vec![],
    };

    let response = script_response(&meta, b"o\xffk");

    assert_eq!((response.code, response.status.as_str()), (201, "Created"));
    assert_eq!(response.headers, [("a".to_string(), "1".to_string())]);
    assert_eq!(response.body, "o\u{fffd}k");
    assert_eq!(response.response_time_ms, 7);
}

#[test]
fn secrets_include_their_base64_forms() {
    let scopes = Scopes::new(vec![layer(
        VarSource::Global,
        &[
            ("k", Value::secret(Some("ab".into()))),
            ("p", Value::plain("x")),
        ],
    )]);

    let secrets = secrets_of(&scopes);

    assert!(secrets.contains(&"ab".to_string()));
    assert!(secrets.contains(&"YWI=".to_string()));
    assert!(secrets.contains(&"YWI".to_string()));
    assert!(!secrets.contains(&"x".to_string()));
}

#[test]
fn secrets_include_their_json_string_form() {
    let scopes = Scopes::new(vec![layer(
        VarSource::Environment,
        &[(
            "key",
            Value {
                text: Some("pa\"ss".into()),
                secret: true,
            },
        )],
    )]);

    assert!(secrets_of(&scopes).contains(&r#"pa\"ss"#.to_string()));
}

#[test]
fn a_script_composed_url_is_masked_with_the_secrets_of_the_scopes() {
    let draft = RequestContent::new("GET", "http://x");
    let request = ScriptRequest {
        method: "GET".into(),
        url: "http://api.test/Tok-991/items?key=Tok-991".into(),
        headers: Vec::new(),
        body: None,
    };

    let resolved = with_secrets(
        Resolved::unchanged(content_of(&request, &draft)),
        &["Tok-991".to_string()],
    );

    assert!(!resolved.loggable_url().contains("Tok-991"));
    assert!(!resolved.redact(&request.url).contains("Tok-991"));
    assert_eq!(
        with_secrets(resolved.clone(), &["Tok-991".to_string()]),
        resolved
    );
}

#[test]
fn a_graphql_body_maps_to_its_json_and_back() {
    let draft = RequestContent {
        body: Body::Graphql {
            query: "{ a }".into(),
            variables: "{\"x\": 1}".into(),
            operation_name: String::new(),
        },
        kind: crate::store::types::RequestKind::Graphql,
        ..RequestContent::new("POST", "http://x")
    };
    let request = script_request(&draft);
    let with = |body: &str| ScriptRequest {
        body: Some(body.into()),
        ..request.clone()
    };

    let unchanged = content_after(&draft, &request);
    let changed = content_after(
        &draft,
        &with(r#"{"query":"{ b }","operationName":"B","variables":{"y":2}}"#),
    );
    let not_graphql = content_after(&draft, &with("[1]"));

    assert_eq!(
        request.body.as_deref(),
        Some(r#"{"query":"{ a }","variables":{"x": 1}}"#)
    );
    assert_eq!(unchanged, draft);
    assert_eq!(
        changed.body,
        Body::Graphql {
            query: "{ b }".into(),
            variables: "{\n  \"y\": 2\n}".into(),
            operation_name: "B".into(),
        }
    );
    assert_eq!(not_graphql.body, Body::Text { text: "[1]".into() });
}

#[test]
fn scripts_of_an_untrusted_collection_do_not_run_until_trusted() {
    use crate::store::types::{RequestLocation, ScriptTarget, Scripts};
    let dir = tempfile::tempdir().unwrap();
    let mut store = crate::store::Store::new(&dir.path().join("app")).unwrap();
    let tree = store.create_collection(dir.path(), "Shop").unwrap();
    let id = tree.collection.id;
    let scripts = Scripts {
        enabled: true,
        pre_request: "pm.environment.set('a','1')".into(),
        post_response: String::new(),
    };
    store
        .write_scripts(
            &ScriptTarget::Collection {
                collection_id: id.clone(),
            },
            &scripts,
        )
        .unwrap();
    let location = RequestLocation::Collection {
        collection_id: id.clone(),
        path: "r.http.yaml".into(),
    };
    let none = Scripts::default();
    assert_eq!(
        collect_chains(&store, &location, &none)
            .unwrap()
            .pre_request
            .len(),
        2
    );
    store.set_scripts_trusted(&id, false).unwrap();
    assert!(collect_chains(&store, &location, &none)
        .unwrap()
        .pre_request
        .is_empty());
    store.set_scripts_trusted(&id, true).unwrap();
    assert_eq!(
        collect_chains(&store, &location, &none)
            .unwrap()
            .pre_request
            .len(),
        2
    );
}
