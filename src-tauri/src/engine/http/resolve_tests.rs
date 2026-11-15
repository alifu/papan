use super::*;
use crate::store::types::{ApiKeyPlacement, ClientAuth, OAuthGrant, Pair};
use crate::vars::dynamic::FixedDynamics;
use crate::vars::scopes::{Layer, Value};
use crate::vars::types::VarSource;

fn scopes() -> Scopes {
    Scopes::new(vec![Layer {
        source: VarSource::Global,
        vars: [
            ("v", Value::plain("V")),
            ("s", Value::secret(Some("S E".into()))),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect(),
    }])
}

fn row(name: &str, value: &str, enabled: bool) -> KeyValue {
    KeyValue {
        name: name.into(),
        value: value.into(),
        enabled,
    }
}

#[test]
fn resolves_what_is_sent_and_nothing_else() {
    let content = RequestContent {
        method: "GET".into(),
        url: "https://{{v}}/{{missing}}".into(),
        disabled_params: vec![Pair {
            name: "{{v}}".into(),
            value: "{{v}}".into(),
        }],
        headers: vec![row("X-{{v}}", "{{s}}", true), row("Off", "{{v}}", false)],
        body: Body::Multipart {
            parts: vec![
                Part {
                    name: "{{v}}".into(),
                    value: "{{v}}".into(),
                    file: None,
                    enabled: true,
                },
                Part {
                    name: "f".into(),
                    value: "{{v}}".into(),
                    file: Some("/tmp/{{v}}".into()),
                    enabled: true,
                },
            ],
        },
        auth: Auth::ApiKey {
            name: "{{v}}".into(),
            value: "{{s}}".into(),
            placement: ApiKeyPlacement::Header,
        },
        ..RequestContent::default()
    };

    let resolved = resolve_content(&content, &scopes(), &FixedDynamics::default());

    let out = &resolved.content;
    assert_eq!(out.url, "https://V/{{missing}}");
    assert_eq!(out.disabled_params, content.disabled_params);
    assert_eq!(out.headers[0], row("X-V", "S E", true));
    assert_eq!(out.headers[1], content.headers[1]);
    let Body::Multipart { parts } = &out.body else {
        panic!()
    };
    assert_eq!(
        (parts[0].name.as_str(), parts[0].value.as_str()),
        ("V", "V")
    );
    assert_eq!(parts[1].file.as_deref(), Some("/tmp/{{v}}"));
    assert_eq!(parts[1].value, "{{v}}");
    assert!(matches!(&out.auth, Auth::ApiKey { name, value, .. } if name == "V" && value == "S E"));
    assert_eq!(resolved.unresolved, ["missing"]);
    assert_eq!(resolved.secret_values, ["S E"]);
    assert_eq!(resolved.secret_headers, ["X-V"]);
}

#[test]
fn bodies_and_auth_kinds_resolve() {
    let dynamics = FixedDynamics::default();
    for body in [
        Body::Json {
            text: "{{v}}".into(),
        },
        Body::Xml {
            text: "{{v}}".into(),
        },
        Body::Text {
            text: "{{v}}".into(),
        },
    ] {
        let content = RequestContent {
            body,
            ..RequestContent::default()
        };
        let out = resolve_content(&content, &scopes(), &dynamics).content.body;
        assert!(
            matches!(out, Body::Json { text } | Body::Xml { text } | Body::Text { text } if text == "V")
        );
    }
    let form = RequestContent {
        body: Body::Form {
            fields: vec![row("{{v}}", "{{v}}", true), row("{{v}}", "", false)],
        },
        auth: Auth::Basic {
            username: "{{v}}".into(),
            password: "{{s}}".into(),
        },
        ..RequestContent::default()
    };
    let out = resolve_content(&form, &scopes(), &dynamics).content;
    let Body::Form { fields } = out.body else {
        panic!()
    };
    assert_eq!(fields[0], row("V", "V", true));
    assert_eq!(fields[1].name, "{{v}}");
    assert_eq!(
        out.auth,
        Auth::Basic {
            username: "V".into(),
            password: "S E".into()
        }
    );
    let bearer = RequestContent {
        auth: Auth::Bearer {
            token: "{{s}}".into(),
        },
        ..RequestContent::default()
    };
    assert_eq!(
        resolve_content(&bearer, &scopes(), &dynamics).content.auth,
        Auth::Bearer {
            token: "S E".into()
        }
    );
}

#[test]
fn digest_and_oauth2_text_fields_resolve_and_choices_do_not() {
    let dynamics = FixedDynamics::default();
    let oauth2 = |text: &str, secret: &str| Auth::Oauth2 {
        grant: OAuthGrant::AuthorizationCode,
        auth_url: format!("https://{text}/a"),
        token_url: format!("https://{text}/t"),
        client_id: text.into(),
        client_secret: secret.into(),
        scope: text.into(),
        client_auth: ClientAuth::Body,
        redirect_port: 8765,
    };
    let digest = RequestContent {
        auth: Auth::Digest {
            username: "{{v}}".into(),
            password: "{{s}}".into(),
        },
        ..RequestContent::default()
    };
    let oauth = RequestContent {
        auth: oauth2("{{v}}", "{{s}}"),
        ..RequestContent::default()
    };

    let digest_out = resolve_content(&digest, &scopes(), &dynamics);
    let oauth_out = resolve_content(&oauth, &scopes(), &dynamics);

    assert_eq!(
        digest_out.content.auth,
        Auth::Digest {
            username: "V".into(),
            password: "S E".into()
        }
    );
    assert_eq!(digest_out.secret_values, ["S E"]);
    assert_eq!(oauth_out.content.auth, oauth2("V", "S E"));
    assert_eq!(oauth_out.secret_values, ["S E"]);
}

#[test]
fn redaction_covers_encoded_forms_and_case() {
    let resolved = Resolved {
        secret_values: vec![
            "a\"b/c".into(),
            "é!x".into(),
            "S E".into(),
            "Host.Example".into(),
            String::new(),
        ],
        ..Resolved::default()
    };

    for (text, expected) in [
        ("p/a%22b/c/q", "p/[REDACTED]/q"),
        ("?k=a%22b%2Fc&z", "?k=[REDACTED]&z"),
        ("u/%C3%A9!x", "u/[REDACTED]"),
        ("u/%c3%a9%21x", "u/[REDACTED]"),
        ("é!x", "[REDACTED]"),
        (
            "a S E b S+E c S%20E",
            "a [REDACTED] b [REDACTED] c [REDACTED]",
        ),
        ("https://host.example/x", "https://[REDACTED]/x"),
        ("100%", "100%"),
    ] {
        assert_eq!(resolved.redact(text), expected, "{text}");
    }
    assert_eq!(
        Resolved::unchanged(RequestContent::default()).redact("x"),
        "x"
    );
    let typed = Resolved::unchanged(RequestContent::new("GET", " {{base}}/a?k={{key}}#f "));
    assert_eq!(typed.loggable_url(), "{{base}}/a");
}

#[test]
fn graphql_query_variables_and_soap_action_resolve_but_operation_name_does_not() {
    use crate::store::types::{RequestKind, SoapSettings, SoapVersion, WsdlRef};
    let gql = RequestContent {
        body: Body::Graphql {
            query: "{ a(x: \"{{v}}\") }".into(),
            variables: "{\"t\": \"{{s}}\", \"e\": \"\\{{v}}\"}".into(),
            operation_name: "{{v}}".into(),
        },
        kind: RequestKind::Graphql,
        ..RequestContent::new("POST", "https://x")
    };
    let soap = RequestContent {
        kind: RequestKind::Soap,
        soap: SoapSettings {
            version: SoapVersion::V11,
            action: "urn:{{s}}".into(),
            wsdl: Some(WsdlRef {
                url: "{{v}}".into(),
                ..WsdlRef::default()
            }),
        },
        ..RequestContent::new("POST", "https://x")
    };

    let gql_resolved = resolve_content(&gql, &scopes(), &FixedDynamics::default());
    let soap_resolved = resolve_content(&soap, &scopes(), &FixedDynamics::default());

    assert_eq!(
        gql_resolved.content.body,
        Body::Graphql {
            query: "{ a(x: \"V\") }".into(),
            variables: "{\"t\": \"S E\", \"e\": \"{{v}}\"}".into(),
            operation_name: "{{v}}".into(),
        }
    );
    assert_eq!(gql_resolved.secret_values, ["S E"]);
    assert_eq!(soap_resolved.content.soap.action, "urn:S E");
    assert_eq!(
        soap_resolved.content.soap.wsdl.as_ref().unwrap().url,
        "{{v}}"
    );
    assert_eq!(soap_resolved.secret_headers, ["SOAPAction"]);
    assert!(!soap_resolved.redact("urn:S E").contains("S E"));
}

#[test]
fn resolves_the_subscription_section_and_records_secrets() {
    let content = RequestContent {
        subscription: SubscriptionSettings {
            url: "wss://{{v}}/ws".into(),
            init_payload: "{ \"token\": \"{{s}}\" }".into(),
        },
        ..RequestContent::default()
    };

    let resolved = resolve_content(&content, &scopes(), &FixedDynamics::default());

    assert_eq!(resolved.content.subscription.url, "wss://V/ws");
    assert_eq!(
        resolved.content.subscription.init_payload,
        "{ \"token\": \"S E\" }"
    );
    assert!(resolved.secret_values.contains(&"S E".to_string()));
}
