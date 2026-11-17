//! Postman auth (v2.0 object and v2.1 list forms, both read by the schema) to our `Auth`
//! (plan 09 §3 "Auth"). Unknown or unsupported kinds become `None` with a report line.

use crate::interop::postman::schema;
use crate::store::types::{ApiKeyPlacement, Auth, ClientAuth, OAuthGrant};

/// Our auth for `auth`, and a report message when something could not be kept.
pub fn map_auth(auth: &schema::Auth) -> (Auth, Option<String>) {
    let text = |key: &str| auth.get(key).unwrap_or_default();
    match auth.kind.as_str() {
        "noauth" | "" => (Auth::None, None),
        "basic" => (
            Auth::Basic {
                username: text("username"),
                password: text("password"),
            },
            None,
        ),
        "digest" => (
            Auth::Digest {
                username: text("username"),
                password: text("password"),
            },
            None,
        ),
        "bearer" => (
            Auth::Bearer {
                token: text("token"),
            },
            None,
        ),
        "apikey" => (api_key(auth), None),
        "oauth2" => oauth2(auth),
        other => (
            Auth::None,
            Some(format!(
                "Auth type `{}` is not supported; the request has no auth.",
                short(other)
            )),
        ),
    }
}

fn api_key(auth: &schema::Auth) -> Auth {
    let placement = match auth.get("in").as_deref() {
        Some("query") => ApiKeyPlacement::Query,
        _ => ApiKeyPlacement::Header,
    };
    Auth::ApiKey {
        name: auth.get("key").unwrap_or_default(),
        value: auth.get("value").unwrap_or_default(),
        placement,
    }
}

fn oauth2(auth: &schema::Auth) -> (Auth, Option<String>) {
    let text = |key: &str| auth.get(key).unwrap_or_default();
    let grant = match auth.get("grant_type").as_deref() {
        Some("client_credentials") => OAuthGrant::ClientCredentials,
        Some("authorization_code") => OAuthGrant::AuthorizationCode,
        Some(other) => {
            return (
                Auth::None,
                Some(format!(
                    "OAuth 2 grant `{}` is not supported; the request has no auth.",
                    short(other)
                )),
            )
        }
        None => {
            return (
                Auth::None,
                Some("OAuth 2 settings without a grant type were dropped.".into()),
            )
        }
    };
    let client_auth = match auth.get("client_authentication").as_deref() {
        Some("body") => ClientAuth::Body,
        _ => ClientAuth::Basic,
    };
    let mapped = Auth::Oauth2 {
        grant,
        auth_url: text("authUrl"),
        token_url: text("accessTokenUrl"),
        client_id: text("clientId"),
        client_secret: text("clientSecret"),
        scope: text("scope"),
        client_auth,
        redirect_port: 0,
    };
    let dropped = ["accessToken", "refreshToken", "password"]
        .iter()
        .any(|key| auth.get(key).is_some_and(|value| !value.is_empty()));
    let note = dropped.then(|| "Stored OAuth 2 tokens and passwords were not imported.".into());
    (mapped, note)
}

/// A kind name for messages, cut so a hostile file cannot flood the report.
fn short(kind: &str) -> String {
    kind.chars().filter(|c| !c.is_control()).take(40).collect()
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;
    use crate::interop::postman::schema::AuthParam;

    fn auth(kind: &str, params: &[(&str, Value)]) -> schema::Auth {
        schema::Auth {
            kind: kind.into(),
            params: params
                .iter()
                .map(|(key, value)| AuthParam {
                    key: (*key).into(),
                    value: value.clone(),
                    kind: None,
                })
                .collect(),
        }
    }

    #[test]
    fn supported_kinds_map_field_by_field() {
        let cases = [
            (auth("noauth", &[]), Auth::None),
            (
                auth(
                    "basic",
                    &[("username", json!("u")), ("password", json!("p"))],
                ),
                Auth::Basic {
                    username: "u".into(),
                    password: "p".into(),
                },
            ),
            (
                auth("digest", &[("username", json!("u"))]),
                Auth::Digest {
                    username: "u".into(),
                    password: String::new(),
                },
            ),
            (
                auth("bearer", &[("token", json!("{{t}}"))]),
                Auth::Bearer {
                    token: "{{t}}".into(),
                },
            ),
            (
                auth(
                    "apikey",
                    &[
                        ("key", json!("k")),
                        ("value", json!(5)),
                        ("in", json!("query")),
                    ],
                ),
                Auth::ApiKey {
                    name: "k".into(),
                    value: "5".into(),
                    placement: ApiKeyPlacement::Query,
                },
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(map_auth(&input), (expected, None), "{}", input.kind);
        }
    }

    #[test]
    fn oauth2_keeps_the_two_grants_and_drops_tokens() {
        let input = auth(
            "oauth2",
            &[
                ("grant_type", json!("authorization_code")),
                ("authUrl", json!("https://a")),
                ("accessTokenUrl", json!("https://t")),
                ("clientId", json!("id")),
                ("clientSecret", json!("s")),
                ("scope", json!("x y")),
                ("client_authentication", json!("body")),
                ("accessToken", json!("tok")),
            ],
        );

        let (mapped, note) = map_auth(&input);

        assert_eq!(
            mapped,
            Auth::Oauth2 {
                grant: OAuthGrant::AuthorizationCode,
                auth_url: "https://a".into(),
                token_url: "https://t".into(),
                client_id: "id".into(),
                client_secret: "s".into(),
                scope: "x y".into(),
                client_auth: ClientAuth::Body,
                redirect_port: 0,
            }
        );
        assert!(note.unwrap().contains("tokens"));
    }

    #[test]
    fn other_grants_and_kinds_become_none_with_a_note() {
        let pkce = auth(
            "oauth2",
            &[("grant_type", json!("authorization_code_with_pkce"))],
        );
        for input in [
            pkce,
            auth("oauth2", &[]),
            auth("awsv4", &[]),
            auth("hawk", &[]),
        ] {
            let (mapped, note) = map_auth(&input);
            assert_eq!(mapped, Auth::None);
            assert!(note.is_some(), "{}", input.kind);
        }
        let long = "x\n".repeat(100);
        let (_, note) = map_auth(&auth(&long, &[]));
        assert!(!note.unwrap().contains('\n'));
    }
}
