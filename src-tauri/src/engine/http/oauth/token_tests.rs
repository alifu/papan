use serde_json::json;

use super::super::fixtures::{self, bearer, CLIENT_ID, CLIENT_SECRET};
use super::*;
use crate::error::ErrorCode;
use crate::store::types::{Auth, OAuthGrant};

const TOKEN: &str = "https://login.example.test/token";

fn config_with(change: impl FnOnce(&mut Auth)) -> OAuthConfig {
    let mut auth = fixtures::oauth2(OAuthGrant::ClientCredentials, "", TOKEN);
    change(&mut auth);
    OAuthConfig::from_resolved(&fixtures::resolved_with(auth))
        .unwrap()
        .unwrap()
}

fn field<'a>(fields: &'a [(&'static str, String)], name: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.as_str())
}

fn body(value: &serde_json::Value) -> Vec<u8> {
    value.to_string().into_bytes()
}

#[test]
fn parse_reads_a_bearer_response() {
    let response = json!({
        "access_token": "at-1", "token_type": "bearer", "expires_in": 3600,
        "refresh_token": "rt-1", "scope": "read"
    });

    let issued = parse(200, &body(&response)).unwrap();

    assert_eq!(issued.access_token.as_str(), "at-1");
    assert_eq!(
        issued.refresh_token.as_deref().map(String::as_str),
        Some("rt-1")
    );
    assert_eq!(issued.expires_in_s, Some(3600));
    assert_eq!(issued.scope.as_deref(), Some("read"));
}

#[test]
fn parse_accepts_any_case_of_bearer_and_optional_fields_absent() {
    for token_type in ["Bearer", "BEARER", "bearer"] {
        let response = json!({ "access_token": "at-1", "token_type": token_type });

        let issued = parse(200, &body(&response)).unwrap();

        assert_eq!(issued.refresh_token, None);
        assert_eq!(issued.expires_in_s, None);
        assert_eq!(issued.scope, None);
    }
}

#[test]
fn parse_reads_expires_in_as_a_number_or_a_numeric_string() {
    let cases = [
        (json!(60), Some(60)),
        (json!("120"), Some(120)),
        (json!(59.9), Some(59)),
        (json!(-5), None),
        (json!("soon"), None),
        (json!(null), None),
    ];

    for (value, expected) in cases {
        let response = json!({ "access_token": "a", "token_type": "bearer", "expires_in": value });

        let issued = parse(200, &body(&response)).unwrap();

        assert_eq!(issued.expires_in_s, expected, "{value}");
    }
}

#[test]
fn parse_refuses_a_token_type_other_than_bearer() {
    let responses = [
        json!({ "access_token": "mac-token-value", "token_type": "mac" }),
        json!({ "access_token": "mac-token-value" }),
        json!({ "access_token": "mac-token-value", "token_type": 7 }),
    ];

    for response in responses {
        let error = parse(200, &body(&response)).unwrap_err();

        assert_eq!(error.code, ErrorCode::InvalidInput);
        assert!(error.message.contains("bearer"));
        assert!(!error.message.contains("mac-token-value"));
    }
}

#[test]
fn parse_refuses_a_missing_token_and_a_body_that_is_not_json() {
    let empty = json!({ "access_token": "", "token_type": "bearer" });
    let missing = json!({ "token_type": "bearer" });

    assert!(parse(200, &body(&empty)).is_err());
    assert!(parse(200, &body(&missing)).is_err());
    let error = parse(200, b"<html>secret-page-text</html>").unwrap_err();
    assert!(!error.message.contains("secret-page-text"));
    assert!(error.detail.is_none());
}

#[test]
fn parse_surfaces_the_whole_provider_error_as_plain_text_for_the_flow_to_bound() {
    let long = "d".repeat(2000);
    let response = json!({
        "error": "invalid_grant",
        "error_description": format!("Bad\r\n<b>grant</b> {long}")
    });

    let error = parse(400, &body(&response)).unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(error
        .message
        .contains("invalid_grant Bad  <b>grant</b> ddd"));
    assert!(error.message.contains(&long));
    assert!(!error.message.chars().any(char::is_control));
}

#[test]
fn parse_reports_the_status_when_an_error_has_no_oauth_body() {
    let error = parse(503, b"upstream-secret-text").unwrap_err();

    assert_eq!(error.message, "The token endpoint answered 503.");
}

#[test]
fn parse_never_treats_a_redirect_as_a_token_response() {
    let response = bearer("at-1", None, None);

    for status in [301, 302, 303, 307, 308] {
        let error = parse(status, &body(&response)).unwrap_err();

        assert!(error.message.contains("redirect"), "{status}");
    }
}

#[test]
fn the_form_of_each_grant_has_exactly_its_fields() {
    let config = config_with(|_| {});

    let credentials = form(&config, Grant::ClientCredentials);
    let code = form(
        &config,
        Grant::Code {
            code: "the-code",
            verifier: "the-verifier",
            redirect_uri: "http://127.0.0.1:4000/callback",
        },
    );
    let refresh = form(
        &config,
        Grant::Refresh {
            refresh_token: "rt-1",
        },
    );

    assert_eq!(
        credentials,
        vec![
            ("grant_type", "client_credentials".to_string()),
            ("scope", "read write".to_string())
        ]
    );
    assert_eq!(field(&code, "grant_type"), Some("authorization_code"));
    assert_eq!(field(&code, "code"), Some("the-code"));
    assert_eq!(field(&code, "code_verifier"), Some("the-verifier"));
    assert_eq!(
        field(&code, "redirect_uri"),
        Some("http://127.0.0.1:4000/callback")
    );
    assert_eq!(code.len(), 4);
    assert_eq!(
        refresh,
        vec![
            ("grant_type", "refresh_token".to_string()),
            ("refresh_token", "rt-1".to_string())
        ]
    );
}

#[test]
fn body_authentication_and_public_clients_put_the_client_in_the_form() {
    let in_body = config_with(|auth| {
        if let Auth::Oauth2 { client_auth, .. } = auth {
            *client_auth = ClientAuth::Body;
        }
    });
    let public = config_with(|auth| {
        if let Auth::Oauth2 { client_secret, .. } = auth {
            client_secret.clear();
        }
    });

    let with_secret = form(&in_body, Grant::ClientCredentials);
    let without_secret = form(&public, Grant::ClientCredentials);

    assert_eq!(field(&with_secret, "client_id"), Some(CLIENT_ID));
    assert_eq!(field(&with_secret, "client_secret"), Some(CLIENT_SECRET));
    assert!(!in_header(&in_body) && !in_header(&public));
    assert_eq!(field(&without_secret, "client_id"), Some(CLIENT_ID));
    assert_eq!(field(&without_secret, "client_secret"), None);
}

#[test]
fn basic_authentication_form_encodes_both_parts_first() {
    let header = basic("client id", "p@ss:word/+");

    let encoded = header.strip_prefix("Basic ").unwrap();
    let decoded = String::from_utf8(STANDARD.decode(encoded).unwrap()).unwrap();
    assert_eq!(decoded, "client+id:p%40ss%3Aword%2F%2B");
}

#[tokio::test]
async fn request_posts_the_form_with_basic_authentication_and_no_cookie() {
    let endpoint = fixtures::endpoint().await;
    endpoint.reply(200, &bearer("at-1", None, Some(60)));
    endpoint.reply(200, &bearer("at-2", None, Some(60)));
    let config = fixtures::config(OAuthGrant::ClientCredentials, "", &endpoint.url());
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let url = config.token_url.clone();

    let first = request(&client, &url, &config, Grant::ClientCredentials).await;
    let second = request(&client, &url, &config, Grant::ClientCredentials).await;

    assert_eq!(first.unwrap().access_token.as_str(), "at-1");
    assert_eq!(second.unwrap().access_token.as_str(), "at-2");
    let hits = endpoint.hits();
    let expected = STANDARD.encode(format!("{CLIENT_ID}:{CLIENT_SECRET}"));
    assert_eq!(
        hits[0].authorization.as_deref(),
        Some(format!("Basic {expected}").as_str())
    );
    assert_eq!(hits[0].accept.as_deref(), Some("application/json"));
    assert_eq!(hits[0].param("client_secret"), None);
    assert_eq!(hits[1].cookie, None);
}

#[tokio::test]
async fn request_does_not_follow_a_redirect() {
    let endpoint = fixtures::endpoint().await;
    endpoint.reply(307, &json!({}));
    endpoint.reply(200, &bearer("at-after-redirect", None, None));
    let config = fixtures::config(OAuthGrant::ClientCredentials, "", &endpoint.url());
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let error = request(
        &client,
        &config.token_url,
        &config,
        Grant::ClientCredentials,
    )
    .await
    .unwrap_err();

    assert!(error.message.contains("redirect"));
    assert_eq!(endpoint.hits().len(), 1);
}

#[tokio::test]
async fn request_refuses_a_response_larger_than_1_mib() {
    let endpoint = fixtures::endpoint().await;
    let padding = "p".repeat(MAX_RESPONSE_BYTES);
    endpoint.reply(
        200,
        &json!({ "access_token": "a", "token_type": "bearer", "x": padding }),
    );
    let config = fixtures::config(OAuthGrant::ClientCredentials, "", &endpoint.url());
    let client = reqwest::Client::new();

    let error = request(
        &client,
        &config.token_url,
        &config,
        Grant::ClientCredentials,
    )
    .await
    .unwrap_err();

    assert!(error.message.contains("1 MiB"));
}

#[tokio::test]
async fn request_reports_an_unreachable_endpoint_as_a_network_error() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/token", listener.local_addr().unwrap());
    drop(listener);
    let config = fixtures::config(OAuthGrant::ClientCredentials, "", &url);

    let error = request(
        &reqwest::Client::new(),
        &config.token_url,
        &config,
        Grant::ClientCredentials,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::Network);
    assert!(!format!("{error:?}").contains(CLIENT_SECRET));
}
