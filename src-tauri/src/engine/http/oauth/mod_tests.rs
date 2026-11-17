use super::fixtures::{oauth2, resolved, resolved_with};
use super::*;
use crate::error::ErrorCode;
use crate::store::types::RequestContent;

const AUTH: &str = "https://login.example.test/authorize";
const TOKEN: &str = "https://login.example.test/token";

fn key_of(change: impl FnOnce(&mut Auth)) -> String {
    let mut auth = oauth2(OAuthGrant::AuthorizationCode, AUTH, TOKEN);
    change(&mut auth);
    config_key(
        &OAuthConfig::from_resolved(&resolved_with(auth))
            .unwrap()
            .unwrap(),
    )
}

#[test]
fn a_draft_without_oauth_has_no_configuration() {
    let resolved = Resolved::unchanged(RequestContent::new("GET", "https://example.test"));

    let config = OAuthConfig::from_resolved(&resolved).unwrap();

    assert!(config.is_none());
}

#[test]
fn the_key_is_64_lowercase_hex_characters_and_stable() {
    let first = key_of(|_| {});
    let second = key_of(|_| {});

    assert_eq!(first, second);
    assert_eq!(first.len(), 64);
    assert!(first
        .chars()
        .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
}

#[test]
fn the_key_changes_with_every_identifying_field() {
    let base = key_of(|_| {});
    let changed = [
        key_of(|auth| {
            if let Auth::Oauth2 { auth_url, .. } = auth {
                *auth_url = "https://other.example.test/authorize".to_string();
            }
        }),
        key_of(|auth| {
            if let Auth::Oauth2 { token_url, .. } = auth {
                *token_url = "https://other.example.test/token".to_string();
            }
        }),
        key_of(|auth| {
            if let Auth::Oauth2 { client_id, .. } = auth {
                *client_id = "another-client".to_string();
            }
        }),
        key_of(|auth| {
            if let Auth::Oauth2 { scope, .. } = auth {
                *scope = "read".to_string();
            }
        }),
        key_of(|auth| {
            if let Auth::Oauth2 { client_auth, .. } = auth {
                *client_auth = ClientAuth::Body;
            }
        }),
        key_of(|auth| {
            if let Auth::Oauth2 { grant, .. } = auth {
                *grant = OAuthGrant::ClientCredentials;
            }
        }),
    ];

    for (index, key) in changed.iter().enumerate() {
        assert_ne!(*key, base, "field {index}");
    }
    let distinct: std::collections::HashSet<&String> = changed.iter().collect();
    assert_eq!(distinct.len(), changed.len());
}

#[test]
fn the_key_ignores_the_client_secret_and_the_redirect_port() {
    let base = key_of(|_| {});

    let other = key_of(|auth| {
        if let Auth::Oauth2 {
            client_secret,
            redirect_port,
            ..
        } = auth
        {
            *client_secret = "a-different-secret".to_string();
            *redirect_port = 8765;
        }
    });

    assert_eq!(other, base);
}

#[test]
fn the_key_encoding_is_length_prefixed() {
    let shifted = key_of(|auth| {
        if let Auth::Oauth2 {
            client_id, scope, ..
        } = auth
        {
            *client_id = "ab".to_string();
            *scope = "c".to_string();
        }
    });
    let other = key_of(|auth| {
        if let Auth::Oauth2 {
            client_id, scope, ..
        } = auth
        {
            *client_id = "a".to_string();
            *scope = "bc".to_string();
        }
    });

    assert_ne!(shifted, other);
}

#[test]
fn client_credentials_ignore_the_authorization_url() {
    let with = resolved(OAuthGrant::ClientCredentials, "file:///etc/passwd", TOKEN);
    let without = resolved(OAuthGrant::ClientCredentials, "", TOKEN);

    let with = OAuthConfig::from_resolved(&with).unwrap().unwrap();
    let without = OAuthConfig::from_resolved(&without).unwrap().unwrap();

    assert!(with.auth_url.is_none());
    assert_eq!(config_key(&with), config_key(&without));
}

#[test]
fn urls_that_are_not_http_with_a_host_are_refused() {
    let bad = [
        "file:///etc/passwd",
        "myapp://callback",
        "javascript:alert(1)",
        "ftp://login.example.test/token",
        "login.example.test/token",
        "http://",
        "https://user:pass@login.example.test/token",
        "",
    ];

    for url in bad {
        let as_token =
            OAuthConfig::from_resolved(&resolved(OAuthGrant::ClientCredentials, "", url));
        let as_auth =
            OAuthConfig::from_resolved(&resolved(OAuthGrant::AuthorizationCode, url, TOKEN));

        assert_eq!(as_token.unwrap_err().code, ErrorCode::InvalidInput, "{url}");
        let error = as_auth.unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{url}");
        assert!(error.message.contains("authorization URL"));
    }
}

#[test]
fn an_unresolved_variable_in_the_configuration_is_named() {
    let unresolved = Resolved {
        unresolved: vec!["idp".to_string(), "elsewhere".to_string()],
        ..resolved(
            OAuthGrant::AuthorizationCode,
            "{{idp}}/authorize",
            "{{idp}}/token",
        )
    };

    let error = OAuthConfig::from_resolved(&unresolved).unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(error.message.contains("`idp`"));
    assert!(!error.message.contains("elsewhere"));
}

#[test]
fn plaintext_is_reported_when_either_endpoint_is_http() {
    let secure = fixtures::config(OAuthGrant::AuthorizationCode, AUTH, TOKEN);
    let auth_plain = fixtures::config(
        OAuthGrant::AuthorizationCode,
        "http://login.example.test/a",
        TOKEN,
    );
    let token_plain = fixtures::config(
        OAuthGrant::ClientCredentials,
        "",
        "http://login.example.test:8080/t",
    );

    assert!(!secure.plaintext());
    assert!(auth_plain.plaintext());
    assert!(token_plain.plaintext());
    assert_eq!(host_of(&token_plain.token_url), "login.example.test:8080");
    assert_eq!(host_of(&secure.token_url), "login.example.test");
}

#[test]
fn debug_output_never_shows_the_client_secret() {
    let config = fixtures::config(OAuthGrant::AuthorizationCode, AUTH, TOKEN);

    let text = format!("{config:?}");

    assert!(!text.contains(fixtures::CLIENT_SECRET));
}
