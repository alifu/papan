//! Pure mapping tests; on-disk export and schema validation are in `tests_disk.rs`.

use serde_json::{json, Value};

use super::auth::auth;
use super::body::body;
use super::url::url;
use super::*;
use crate::store::env_model::{EnvFile, GlobalsFile, VarEntry, VarTable};
use crate::store::types::{
    ApiKeyPlacement, Auth, Body, ClientAuth, KeyValue, OAuthGrant, Pair, Part, RequestContent,
    RequestKind, Scripts, Settings, SoapSettings, SoapVersion, SubscriptionSettings,
};

mod disk;

fn val<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).expect("json")
}

fn kv(name: &str, value: &str, enabled: bool) -> KeyValue {
    KeyValue {
        name: name.into(),
        value: value.into(),
        enabled,
    }
}

#[test]
fn url_keeps_raw_and_splits_parts_with_disabled_params() {
    let off = [Pair {
        name: "x".into(),
        value: "1".into(),
    }];
    let u = url("https://api.io:8443/v1/users/?q=a&flag#top", &off).expect("url");
    assert_eq!(
        val(&u),
        json!({"raw": "https://api.io:8443/v1/users/?q=a&flag#top", "protocol": "https",
            "host": ["api", "io"], "port": "8443", "path": ["v1", "users", ""],
            "query": [{"key": "q", "value": "a"}, {"key": "flag", "value": null},
                      {"key": "x", "value": "1", "disabled": true}], "hash": "top"})
    );
    let v = val(&url("{{base}}/a", &[]).expect("url"));
    assert_eq!(
        v,
        json!({"raw": "{{base}}/a", "host": ["{{base}}"], "path": ["a"]})
    );
    assert!(url("", &[]).is_none());
}

#[test]
fn auth_maps_every_kind_in_key_order() {
    let mut report = Report::new("c.json");
    let p = |a: &Auth, r: &mut Report| auth(a, "r", r).map(|a| val(&a));
    let s = |k: &str, v: &str| json!({"key": k, "value": v, "type": "string"});
    let basic = Auth::Basic {
        username: "u".into(),
        password: "{{pw}}".into(),
    };
    assert_eq!(
        p(&basic, &mut report),
        Some(json!({"type": "basic", "basic": [s("password", "{{pw}}"), s("username", "u")]}))
    );
    let key = Auth::ApiKey {
        name: "X-Key".into(),
        value: "{{k}}".into(),
        placement: ApiKeyPlacement::Query,
    };
    assert_eq!(
        p(&key, &mut report),
        Some(
            json!({"type": "apikey", "apikey": [s("in", "query"), s("key", "X-Key"), s("value", "{{k}}")]})
        )
    );
    let bearer = Auth::Bearer {
        token: "{{t}}".into(),
    };
    assert_eq!(
        p(&bearer, &mut report),
        Some(json!({"type": "bearer", "bearer": [s("token", "{{t}}")]}))
    );
    let oauth = |grant| Auth::Oauth2 {
        grant,
        auth_url: "https://a".into(),
        token_url: "https://t".into(),
        client_id: "id".into(),
        client_secret: "{{cs}}".into(),
        scope: "s".into(),
        client_auth: ClientAuth::Body,
        redirect_port: 0,
    };
    let cc = p(&oauth(OAuthGrant::ClientCredentials), &mut report).expect("oauth");
    let keys: Vec<&str> = cc["oauth2"]
        .as_array()
        .expect("list")
        .iter()
        .filter_map(|x| x["key"].as_str())
        .collect();
    assert_eq!(
        keys,
        [
            "accessTokenUrl",
            "addTokenTo",
            "client_authentication",
            "clientId",
            "clientSecret",
            "grant_type",
            "scope"
        ]
    );
    let code = p(&oauth(OAuthGrant::AuthorizationCode), &mut report).expect("oauth");
    assert!(code["oauth2"]
        .as_array()
        .expect("list")
        .iter()
        .any(|x| x["key"] == "authUrl"));
    assert_eq!(p(&Auth::None, &mut report), Some(json!({"type": "noauth"})));
    assert!(report.items.is_empty());
    assert_eq!(p(&Auth::Unsupported, &mut report), None);
    assert_eq!(report.items[0].level, ReportLevel::Skipped);
}

#[test]
fn body_maps_every_mode() {
    let mut report = Report::new("c.json");
    let b = |body_: &Body, r: &mut Report| body(body_, "r", r).map(|b| val(&b));
    assert_eq!(
        b(&Body::Json { text: "{}".into() }, &mut report),
        Some(json!({"mode": "raw", "raw": "{}", "options": {"raw": {"language": "json"}}}))
    );
    assert_eq!(
        b(
            &Body::Form {
                fields: vec![kv("a", "1", false)]
            },
            &mut report
        ),
        Some(
            json!({"mode": "urlencoded", "urlencoded": [{"key": "a", "value": "1", "disabled": true}]})
        )
    );
    let parts = vec![
        Part {
            name: "f".into(),
            value: String::new(),
            file: Some("/tmp/a.png".into()),
            enabled: true,
        },
        Part {
            name: "t".into(),
            value: "v".into(),
            file: None,
            enabled: true,
        },
    ];
    assert_eq!(
        b(&Body::Multipart { parts }, &mut report),
        Some(
            json!({"mode": "formdata", "formdata": [{"key": "f", "src": "/tmp/a.png", "type": "file"},
            {"key": "t", "value": "v", "type": "text"}]})
        )
    );
    assert_eq!(
        b(
            &Body::Binary {
                file: Some("/b".into())
            },
            &mut report
        ),
        Some(json!({"mode": "file", "file": {"src": "/b"}}))
    );
    let gql = Body::Graphql {
        query: "{a}".into(),
        variables: "{}".into(),
        operation_name: String::new(),
    };
    assert_eq!(
        b(&gql, &mut report),
        Some(json!({"mode": "graphql", "graphql": {"query": "{a}", "variables": "{}"}}))
    );
    assert!(report.items.is_empty());
    let named = Body::Graphql {
        query: "q".into(),
        variables: String::new(),
        operation_name: "Op".into(),
    };
    assert!(b(&named, &mut report).is_some());
    assert_eq!(b(&Body::None, &mut report), None);
    assert_eq!(b(&Body::Unsupported, &mut report), None);
    assert_eq!(report.items.len(), 2);
}

fn content(kind: RequestKind) -> RequestContent {
    RequestContent {
        kind,
        ..RequestContent::new("POST", "https://x.io")
    }
}

#[test]
fn ws_and_grpc_are_skipped_with_a_report() {
    let mut report = Report::new("c.json");
    for kind in [RequestKind::Ws, RequestKind::Grpc] {
        assert!(request_item("r", &content(kind), None, "r.http.yaml", &mut report).is_none());
    }
    let items = report.into_items();
    assert_eq!(items.len(), 2);
    assert!(items
        .iter()
        .all(|i| i.level == ReportLevel::Skipped && i.file == "c.json"));
}

#[test]
fn soap_adds_version_headers_unless_present() {
    let mut report = Report::new("c.json");
    let soap = |version, headers: Vec<KeyValue>| RequestContent {
        soap: SoapSettings {
            version,
            action: "urn:Get".into(),
            wsdl: None,
        },
        headers,
        body: Body::Xml {
            text: "<e/>".into(),
        },
        ..content(RequestKind::Soap)
    };
    let headers = |c: &RequestContent, r: &mut Report| {
        val(&request_item("s", c, None, "s", r)
            .expect("item")
            .request
            .header)
    };
    assert_eq!(
        headers(&soap(SoapVersion::V11, vec![]), &mut report),
        json!([{"key": "Content-Type", "value": "text/xml; charset=utf-8"},
               {"key": "SOAPAction", "value": "\"urn:Get\""}])
    );
    assert_eq!(
        headers(
            &soap(SoapVersion::V12, vec![kv("X", "1", true)]),
            &mut report
        ),
        json!([{"key": "X", "value": "1"},
               {"key": "Content-Type", "value": "application/soap+xml; charset=utf-8; action=\"urn:Get\""}])
    );
    let own = soap(SoapVersion::V11, vec![kv("content-type", "x", true)]);
    assert_eq!(headers(&own, &mut report).as_array().map(Vec::len), Some(2));
}

#[test]
fn request_item_writes_scripts_settings_and_reports_lossy_fields() {
    let mut report = Report::new("c.json");
    let c = RequestContent {
        headers: vec![kv("A", "1", false)],
        scripts: Scripts {
            enabled: true,
            pre_request: "a();\nb();".into(),
            post_response: String::new(),
        },
        settings: Settings {
            follow_redirects: false,
            timeout_ms: 5,
            ..Settings::default()
        },
        subscription: SubscriptionSettings {
            url: "wss://x".into(),
            init_payload: String::new(),
        },
        ..content(RequestKind::Graphql)
    };
    let item = request_item("Get", &c, Some("doc".into()), "g", &mut report).expect("item");
    assert_eq!(
        val(&item),
        json!({"name": "Get",
            "event": [{"listen": "prerequest", "script": {"type": "text/javascript", "exec": ["a();", "b();"]}}],
            "protocolProfileBehavior": {"followRedirects": false},
            "request": {"method": "POST", "auth": {"type": "noauth"}, "header": [{"key": "A", "value": "1", "disabled": true}],
                "url": {"raw": "https://x.io", "protocol": "https", "host": ["x", "io"]},
                "description": "doc"}})
    );
    request_item("Get", &c, None, "g2", &mut report);
    let messages: Vec<String> = report.into_items().into_iter().map(|i| i.message).collect();
    assert_eq!(
        messages.iter().filter(|m| m.contains("timeouts")).count(),
        1
    );
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.contains("subscription"))
            .count(),
        2
    );
}

fn table() -> VarTable {
    let mut stored_secret = VarEntry::new(String::new(), true, true);
    // Even if a file somehow held a secret value, it must never be exported.
    stored_secret.value = "leaked".into();
    VarTable::from([
        (
            "host".to_string(),
            VarEntry::new("x.io".into(), false, false),
        ),
        ("token".to_string(), stored_secret),
    ])
}

#[test]
fn variables_blank_secrets_unless_values_are_passed() {
    let blank = val(&collection_variables(&table(), None));
    assert_eq!(
        blank,
        json!([{"key": "host", "value": "x.io", "disabled": true},
               {"key": "token", "value": "", "type": "string"}])
    );
    let secrets = SecretValues::from([("token".to_string(), "s3".to_string())]);
    let with = collection_variables(&table(), Some(&secrets));
    assert_eq!(with[1].value, "s3");
}

#[test]
fn environment_and_globals_files() {
    let env = EnvFile {
        variables: table(),
        ..EnvFile::new("Dev".into())
    };
    assert_eq!(
        val(&environment(&env, "fallback", None)),
        json!({"id": "fallback", "name": "Dev", "values": [
            {"key": "host", "value": "x.io", "type": "default", "enabled": false},
            {"key": "token", "value": "", "type": "secret", "enabled": true}],
            "_postman_variable_scope": "environment"})
    );
    let mut imported = env.clone();
    imported.extra.insert("postman_id".into(), json!("pm-1"));
    assert_eq!(
        environment(&imported, "fallback", None).id.as_deref(),
        Some("pm-1")
    );
    let g = globals(
        &GlobalsFile {
            variables: table(),
            ..GlobalsFile::default()
        },
        None,
    );
    assert_eq!(
        (g.name.as_str(), g.scope.as_deref(), g.id),
        ("Globals", Some("globals"), None)
    );
}

#[test]
fn to_json_is_tab_indented_with_trailing_newline() {
    let text = to_json(&json!({"a": [1]})).expect("json");
    assert_eq!(text, "{\n\t\"a\": [\n\t\t1\n\t]\n}\n");
}
