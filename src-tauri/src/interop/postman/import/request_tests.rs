use serde_json::json;

use super::*;
use crate::interop::postman::schema::{AuthParam, GraphqlBody, Header, QueryParam, Url};
use crate::store::types::Scripts;

const AT: At<'static> = At {
    file: "c.json",
    path: "item[0]",
};

fn item(request: schema::Request) -> schema::RequestItem {
    schema::RequestItem {
        name: "R".into(),
        event: Vec::new(),
        protocol_profile_behavior: None,
        request,
        response_count: 0,
    }
}

fn bearer(token: &str) -> schema::Auth {
    schema::Auth {
        kind: "bearer".into(),
        params: vec![AuthParam {
            key: "token".into(),
            value: json!(token),
            kind: None,
        }],
    }
}

#[test]
fn url_headers_and_disabled_query_map() {
    let request = schema::Request {
        method: Some("post".into()),
        header: vec![Header {
            key: "X".into(),
            value: "1".into(),
            disabled: true,
            description: None,
        }],
        url: Some(Url {
            raw: Some("https://a/b?x=1".into()),
            query: vec![
                QueryParam {
                    key: Some("x".into()),
                    value: Some("1".into()),
                    ..QueryParam::default()
                },
                QueryParam {
                    key: Some("y".into()),
                    value: Some("2".into()),
                    disabled: true,
                    description: None,
                },
            ],
            ..Url::default()
        }),
        ..schema::Request::default()
    };
    let mut report = Report::new();

    let content = map_request(AT, &item(request), None, &mut report);

    assert_eq!(content.method, "POST");
    assert_eq!(content.url, "https://a/b?x=1");
    assert_eq!(
        content.disabled_params,
        vec![Pair {
            name: "y".into(),
            value: "2".into()
        }]
    );
    assert!(!content.headers[0].enabled);
    assert_eq!(content.kind, RequestKind::Http);
}

#[test]
fn url_is_rebuilt_from_parts_without_raw() {
    let url = Url {
        protocol: Some("https".into()),
        host: vec!["api".into(), "example".into(), "com".into()],
        port: Some("8443".into()),
        path: vec!["v1".into(), ":id".into()],
        query: vec![QueryParam {
            key: Some("q".into()),
            value: Some("a".into()),
            ..QueryParam::default()
        }],
        ..Url::default()
    };

    assert_eq!(url_text(&url), "https://api.example.com:8443/v1/:id?q=a");
}

#[test]
fn own_auth_wins_and_inherited_auth_is_copied() {
    let mut report = Report::new();
    let own = schema::Request {
        auth: Some(bearer("own")),
        ..schema::Request::default()
    };
    let none = schema::Request {
        auth: Some(schema::Auth {
            kind: "noauth".into(),
            params: Vec::new(),
        }),
        ..schema::Request::default()
    };
    let parent = bearer("parent");

    let a = map_request(AT, &item(own), Some(&parent), &mut report);
    let b = map_request(
        AT,
        &item(schema::Request::default()),
        Some(&parent),
        &mut report,
    );
    let c = map_request(AT, &item(none), Some(&parent), &mut report);

    assert_eq!(
        a.auth,
        Auth::Bearer {
            token: "own".into()
        }
    );
    assert_eq!(
        b.auth,
        Auth::Bearer {
            token: "parent".into()
        }
    );
    assert_eq!(c.auth, Auth::None);
}

#[test]
fn ws_urls_and_graphql_bodies_pick_the_kind() {
    let mut report = Report::new();
    let ws = schema::Request {
        method: Some("POST".into()),
        url: Some(Url {
            raw: Some("wss://x/socket".into()),
            ..Url::default()
        }),
        body: Some(schema::Body {
            mode: Some("raw".into()),
            raw: Some("hi".into()),
            ..schema::Body::default()
        }),
        ..schema::Request::default()
    };
    let graphql = schema::Request {
        method: Some("GET".into()),
        body: Some(schema::Body {
            mode: Some("graphql".into()),
            graphql: Some(GraphqlBody {
                query: "{a}".into(),
                variables: String::new(),
            }),
            ..schema::Body::default()
        }),
        ..schema::Request::default()
    };

    let ws = map_request(AT, &item(ws), None, &mut report);
    let graphql = map_request(AT, &item(graphql), None, &mut report);

    assert_eq!(
        (ws.kind, ws.method.as_str(), &ws.body),
        (RequestKind::Ws, "GET", &Body::None)
    );
    assert_eq!(
        (graphql.kind, graphql.method.as_str()),
        (RequestKind::Graphql, "POST")
    );
    assert!(request_file("W", &ws, None).is_ok());
    assert!(request_file("G", &graphql, None).is_ok());
}

#[test]
fn behaviour_settings_and_losses_are_reported() {
    let mut report = Report::new();
    let behaviour = json!({"followRedirects": false, "strictSSL": false, "disableCookies": true});
    let with = schema::RequestItem {
        protocol_profile_behavior: behaviour.as_object().cloned(),
        response_count: 2,
        ..item(schema::Request::default())
    };

    let content = map_request(AT, &with, None, &mut report);

    assert!(!content.settings.follow_redirects);
    assert!(!content.settings.verify_tls);
    assert_eq!(report.items().len(), 2);
}

#[test]
fn request_file_keeps_description_and_switches_scripts_off() {
    let content = RequestContent {
        scripts: Scripts {
            enabled: true,
            pre_request: "pm.test('a', () => {});".into(),
            post_response: String::new(),
        },
        ..RequestContent::new("GET", "https://x")
    };

    let file = request_file("Ping", &content, Some("Docs")).unwrap();
    let text = file.to_yaml().unwrap();
    let back = RequestFile::parse("p.http.yaml", text.as_bytes()).unwrap();

    assert!(text.contains("enabled: false"), "{text}");
    assert!(text.contains("description: Docs"), "{text}");
    let expected = Scripts {
        enabled: false,
        ..content.scripts.clone()
    };
    assert_eq!(back.content().unwrap().scripts, expected);
}

#[test]
fn invalid_content_is_refused_by_validation() {
    let content = RequestContent::new("GE T", "https://x");

    assert!(request_file("Bad", &content, None).is_err());
}
