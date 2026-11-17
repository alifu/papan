use serde_json::json;

use super::*;

fn coll(v: serde_json::Value) -> Parsed<Collection> {
    parse_collection(&v).expect("parses")
}

fn err(v: serde_json::Value) -> String {
    parse_collection(&v).expect_err("fails").to_string()
}

fn only_request(c: &Collection) -> &RequestItem {
    match &c.item[0] {
        Item::Request(r) => r.as_ref(),
        Item::Folder(_) => panic!("expected a request"),
    }
}

#[test]
fn reads_v21_collection_with_folder_request_and_url_object() {
    let c = coll(json!({
        "info": {"_postman_id": "abc", "name": "Demo", "description": {"content": "Hi"},
                 "schema": SCHEMA_V21},
        "item": [{"name": "F", "item": [{"name": "Get", "request": {
            "method": "GET",
            "header": [{"key": "Accept", "value": "*/*", "disabled": true}],
            "url": {"raw": "https://x.io/:id?q=1", "protocol": "https", "host": ["x", "io"],
                    "port": 8080, "path": ["", {"type": "string", "value": ":id"}],
                    "query": [{"key": "q", "value": "1"}, {"key": null, "value": null}],
                    "variable": [{"key": "id", "value": 5}]}
        }, "response": [{}, {}]}]}],
        "variable": [{"id": "legacy", "value": true}]
    }));
    assert_eq!(c.unknown_fields, 0);
    let c = c.value;
    assert_eq!(c.info.postman_id.as_deref(), Some("abc"));
    assert_eq!(c.info.description.as_deref(), Some("Hi"));
    assert_eq!(c.variable[0].key, "legacy");
    assert_eq!(c.variable[0].value, "true");
    let Item::Folder(f) = &c.item[0] else {
        panic!("folder")
    };
    let Item::Request(r) = &f.item[0] else {
        panic!("request")
    };
    assert_eq!(r.response_count, 2);
    assert!(r.request.header[0].disabled);
    let url = r.request.url.as_ref().expect("url");
    assert_eq!(url.port.as_deref(), Some("8080"));
    assert_eq!(url.path, vec!["".to_string(), ":id".to_string()]);
    assert_eq!(url.query[1].key, None);
    assert_eq!(url.variable[0].value, "5");
}

#[test]
fn reads_string_request_string_url_and_string_headers() {
    let c = coll(json!({"info": {"name": "n"}, "item": [
        {"name": "a", "request": "https://a.io"},
    ]}));
    let r = only_request(&c.value);
    assert_eq!(
        r.request.url.as_ref().and_then(|u| u.raw.as_deref()),
        Some("https://a.io")
    );

    let c = coll(
        json!({"info": {"name": "n"}, "item": [{"name": "b", "request": {
        "url": "https://b.io", "header": "A: 1\nB:2\n\nC"}}]}),
    );
    let h = &only_request(&c.value).request.header;
    assert_eq!(h.len(), 3);
    assert_eq!((h[1].key.as_str(), h[1].value.as_str()), ("B", "2"));
    assert_eq!((h[2].key.as_str(), h[2].value.as_str()), ("C", ""));
}

#[test]
fn reads_v20_object_auth_and_v21_list_auth_the_same() {
    let v20 = coll(json!({"info": {"name": "n"}, "item": [], "auth": {
        "type": "basic", "basic": {"username": "u", "password": "p"}}}));
    let v21 = coll(json!({"info": {"name": "n"}, "item": [], "auth": {
        "type": "basic", "basic": [{"key": "password", "value": "p", "type": "string"},
                                    {"key": "username", "value": "u", "type": "string"}],
        "bearer": [{"key": "token", "value": "ignored"}]}}));
    for c in [&v20.value, &v21.value] {
        let a = c.auth.as_ref().expect("auth");
        assert_eq!(a.kind, "basic");
        assert_eq!(a.get("username").as_deref(), Some("u"));
        assert_eq!(a.get("password").as_deref(), Some("p"));
    }
    assert_eq!(v21.unknown_fields, 0);
}

#[test]
fn reads_bodies_events_and_formdata_src_forms() {
    let c = coll(json!({"info": {"name": "n"}, "item": [{"name": "r",
        "event": [{"listen": "test", "script": {"type": "text/javascript", "exec": ["a", "b"]}},
                  {"listen": "prerequest", "script": {"exec": "single"}}],
        "request": {"body": {"mode": "formdata", "formdata": [
            {"key": "f", "type": "file", "src": ["/a", "/b"]},
            {"key": "g", "type": "file", "src": "/c"},
            {"key": "t", "value": "v", "type": "text", "contentType": "text/plain"}],
            "options": {"raw": {"language": "json"}}, "graphql": {"query": "{a}",
            "variables": {"x": 1}}, "file": {"src": "/bin"}}}}]}));
    let r = only_request(&c.value);
    assert_eq!(r.event[0].script.source(), "a\nb");
    assert_eq!(r.event[1].script.exec, vec!["single".to_string()]);
    let b = r.request.body.as_ref().expect("body");
    assert_eq!(b.formdata[0].src.len(), 2);
    assert_eq!(b.formdata[1].src, vec!["/c".to_string()]);
    assert_eq!(b.formdata[2].content_type.as_deref(), Some("text/plain"));
    assert_eq!(
        b.options.as_ref().map(|o| o.raw.language.as_str()),
        Some("json")
    );
    assert_eq!(
        b.graphql.as_ref().map(|g| g.variables.as_str()),
        Some(r#"{"x":1}"#)
    );
    assert_eq!(b.file.as_ref().and_then(|f| f.src.as_deref()), Some("/bin"));
}

#[test]
fn counts_unknown_fields_and_treats_null_as_absent() {
    let c = coll(
        json!({"info": {"name": "n", "zzz": 1}, "item": [{"name": "r", "foo": 1,
        "request": {"url": null, "body": null, "description": null, "bar": [1]}}], "top": {}}),
    );
    assert_eq!(c.unknown_fields, 4);
    assert!(only_request(&c.value).request.url.is_none());
}

#[test]
fn wrong_types_name_the_json_path() {
    let cases = [
        (json!([]), "the file must be an object."),
        (json!({"item": []}), "field `info` must be an object."),
        (
            json!({"info": {"name": 5}}),
            "field `info.name` must be a string.",
        ),
        (
            json!({"info": {}, "item": {}}),
            "field `item` must be an array.",
        ),
        (
            json!({"info": {}, "item": [{"name": "r", "request": {"url": 5}}]}),
            "field `item[0].request.url` must be an object.",
        ),
        (
            json!({"info": {}, "item": [{"item": [{"name": "x"}]}]}),
            "field `item[0].item[0]` must be a request (with `request`) or a folder (with `item`).",
        ),
        (
            json!({"info": {}, "item": [{"request": {"header": [{"key": "a", "disabled": "y"}]}}]}),
            "field `item[0].request.header[0].disabled` must be a boolean.",
        ),
        (
            json!({"info": {}, "item": [], "event": [{"script": {"exec": ["a", 3]}}]}),
            "field `event[0].script.exec[1]` must be a string.",
        ),
        (
            json!({"info": {}, "item": [], "auth": {"basic": []}}),
            "field `auth.type` must be a string.",
        ),
        (
            json!({"info": {}, "item": [], "auth": {"type": "bearer", "bearer": [{"value": 1}]}}),
            "field `auth.bearer[0].key` must be a string.",
        ),
    ];
    for (input, message) in cases {
        assert_eq!(err(input), message);
    }
}

#[test]
fn reads_environment_and_globals_with_defaults() {
    let env = parse_variable_file(&json!({"id": "e1", "name": "Dev", "values": [
        {"key": "host", "value": "x", "type": "default", "enabled": true},
        {"key": "token", "value": "s", "type": "secret"},
        {"key": "off", "value": 3, "enabled": false}],
        "_postman_variable_scope": "environment", "_postman_exported_at": "t"}))
    .expect("env");
    assert_eq!(env.unknown_fields, 0);
    let v = &env.value.values;
    assert!(v[1].enabled);
    assert_eq!(v[1].kind.as_deref(), Some("secret"));
    assert_eq!((v[2].value.as_str(), v[2].enabled), ("3", false));

    let globals = parse_variable_file(&json!({"values": [], "_postman_variable_scope": "globals"}))
        .expect("globals");
    assert_eq!(globals.value.scope.as_deref(), Some("globals"));
    let bad = parse_variable_file(&json!({"values": [{"key": 1}]})).expect_err("bad");
    assert_eq!(bad.to_string(), "field `values[0].key` must be a string.");
}

#[test]
fn serialises_v21_shape_in_fixed_key_order() {
    let c = Collection {
        info: Info {
            postman_id: Some("id".into()),
            name: "N".into(),
            description: None,
            schema: SCHEMA_V21.into(),
        },
        item: vec![Item::Request(Box::new(RequestItem {
            name: "r".into(),
            event: vec![],
            protocol_profile_behavior: None,
            request: Request {
                auth: Some(Auth {
                    kind: "noauth".into(),
                    params: vec![],
                }),
                method: Some("GET".into()),
                body: Some(Body {
                    mode: Some("formdata".into()),
                    formdata: vec![FormParam {
                        key: "f".into(),
                        src: vec!["/a".into()],
                        kind: Some("file".into()),
                        ..FormParam::default()
                    }],
                    ..Body::default()
                }),
                ..Request::default()
            },
            response_count: 3,
        }))],
        auth: Some(Auth {
            kind: "bearer".into(),
            params: vec![AuthParam {
                key: "token".into(),
                value: json!("{{t}}"),
                kind: Some("string".into()),
            }],
        }),
        event: vec![],
        variable: vec![],
        protocol_profile_behavior: None,
    };
    let text = serde_json::to_string(&c).expect("json");
    assert_eq!(
        text,
        concat!(
            r#"{"info":{"_postman_id":"id","name":"N","schema":""#,
            "https://schema.getpostman.com/json/collection/v2.1.0/collection.json",
            r#""},"item":[{"name":"r","request":{"auth":{"type":"noauth"},"method":"GET","#,
            r#""body":{"mode":"formdata","formdata":[{"key":"f","src":"/a","type":"file"}]}}}],"#,
            r#""auth":{"type":"bearer","bearer":[{"key":"token","value":"{{t}}","type":"string"}]}}"#
        )
    );
    let back = coll(serde_json::from_str(&text).expect("value"));
    assert_eq!(only_request(&back.value).request, only_request(&c).request);
}
