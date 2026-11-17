use serde_json::json;

use super::*;
use crate::store::request_emit::{canonical_extra, emit_section, normalise};
use crate::store::request_model::{parse_sections, validate_content, Extra};
use crate::store::types::{HttpVersion, RequestContent, RequestKind, Settings};

fn extra(value: Value) -> Extra {
    value
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// The non-URL fields of an OAuth 2 section that the tests vary.
struct Extras<'a> {
    client_secret: &'a str,
    scope: &'a str,
    client_auth: ClientAuth,
    redirect_port: u16,
}

const DEFAULTS: Extras = Extras {
    client_secret: "",
    scope: "",
    client_auth: ClientAuth::Basic,
    redirect_port: 0,
};

fn oauth2_with(grant: OAuthGrant, extras: Extras) -> Auth {
    Auth::Oauth2 {
        grant,
        auth_url: "{{idp}}/authorize".into(),
        token_url: "{{idp}}/token".into(),
        client_id: "papan".into(),
        client_secret: extras.client_secret.into(),
        scope: extras.scope.into(),
        client_auth: extras.client_auth,
        redirect_port: extras.redirect_port,
    }
}

fn oauth2(grant: OAuthGrant) -> Auth {
    oauth2_with(grant, DEFAULTS)
}

fn with_auth(auth: Auth) -> RequestContent {
    RequestContent {
        auth,
        ..RequestContent::new("GET", "https://x")
    }
}

#[test]
fn oauth2_omitted_fields_take_their_defaults() {
    let raw = extra(json!({"auth": {
        "type": "oauth2", "grant": "authorization_code", "client_id": "papan",
        "auth_url": "{{idp}}/authorize", "token_url": "{{idp}}/token",
    }}));

    let content = parse_sections("r", RequestKind::Http, &raw).unwrap();

    assert_eq!(content.auth, oauth2(OAuthGrant::AuthorizationCode));
}

#[test]
fn explicit_defaults_are_omitted_on_emit() {
    let raw = extra(json!({
        "auth": {
            "type": "oauth2", "grant": "client_credentials", "token_url": "t",
            "client_auth": "basic", "redirect_port": 0, "scope": "", "client_secret": "",
            "auth_url": "ignored",
        },
        "settings": {"http_version": "auto"},
    }));

    let canonical = canonical_extra("r", RequestKind::Http, &raw).unwrap();

    assert_eq!(
        canonical["auth"],
        json!({"type": "oauth2", "grant": "client_credentials", "token_url": "t"})
    );
    assert!(!canonical.contains_key("settings"));
}

#[test]
fn every_non_default_field_is_written() {
    let content = with_auth(oauth2_with(
        OAuthGrant::AuthorizationCode,
        Extras {
            client_secret: "{{s}}",
            scope: "read",
            client_auth: ClientAuth::Body,
            redirect_port: 8765,
        },
    ));

    let value = emit_section("auth", &content, None).unwrap();

    assert_eq!(
        value,
        json!({
            "type": "oauth2", "grant": "authorization_code", "auth_url": "{{idp}}/authorize",
            "token_url": "{{idp}}/token", "client_id": "papan", "client_secret": "{{s}}",
            "scope": "read", "client_auth": "body", "redirect_port": 8765,
        })
    );
}

#[test]
fn digest_and_http_version_round_trip() {
    let raw = extra(json!({
        "auth": {"type": "digest", "username": "u", "password": "{{p}}"},
        "settings": {"http_version": "http1"},
    }));

    let content = parse_sections("r", RequestKind::Http, &raw).unwrap();
    let canonical = canonical_extra("r", RequestKind::Http, &raw).unwrap();

    assert_eq!(
        content.auth,
        Auth::Digest {
            username: "u".into(),
            password: "{{p}}".into()
        }
    );
    assert_eq!(
        content.settings,
        Settings {
            http_version: HttpVersion::Http1,
            ..Settings::default()
        }
    );
    assert_eq!(canonical, raw);
}

#[test]
fn shape_errors_name_the_field() {
    let base = json!({"type": "oauth2", "grant": "client_credentials", "token_url": "t"});
    let cases = [
        (json!({"grant": "password"}), "auth.grant"),
        (json!({"client_auth": "jwt"}), "auth.client_auth"),
        (json!({"redirect_port": 1023}), "auth.redirect_port"),
        (json!({"redirect_port": 70000}), "auth.redirect_port"),
        (json!({"redirect_port": "8765"}), "auth.redirect_port"),
        (json!({"grant": "authorization_code"}), "auth.auth_url"),
        (json!({"client_id": "x".repeat(513)}), "auth.client_id"),
        (json!({"token_url": "x".repeat(2049)}), "auth.token_url"),
    ];
    for (patch, field) in cases {
        let mut auth = base.as_object().unwrap().clone();
        auth.extend(patch.as_object().unwrap().clone());
        let raw = extra(json!({"auth": auth}));

        let error = parse_sections("r", RequestKind::Http, &raw).unwrap_err();

        assert!(error.message.contains(field), "{field}: {}", error.message);
    }
}

#[test]
fn webview_content_is_held_to_the_same_limits() {
    let low_port = with_auth(oauth2_with(
        OAuthGrant::AuthorizationCode,
        Extras {
            redirect_port: 80,
            ..DEFAULTS
        },
    ));
    let scope = "s".repeat(MAX_SCOPE_CHARS + 1);
    let long_scope = with_auth(oauth2_with(
        OAuthGrant::ClientCredentials,
        Extras {
            scope: &scope,
            ..DEFAULTS
        },
    ));
    let long_password = with_auth(Auth::Digest {
        username: "u".into(),
        password: "p".repeat(MAX_ENTRY_VALUE_CHARS + 1),
    });

    let errors = [low_port, long_scope, long_password]
        .map(|content| validate_content("r", &content).unwrap_err().message);

    assert!(errors[0].contains("auth.redirect_port"), "{}", errors[0]);
    assert!(errors[1].contains("auth.scope"), "{}", errors[1]);
    assert!(errors[2].contains("auth.password"), "{}", errors[2]);
}

#[test]
fn normalise_drops_the_auth_url_of_client_credentials_only() {
    let client = normalise(&with_auth(oauth2(OAuthGrant::ClientCredentials)));
    let code = normalise(&with_auth(oauth2(OAuthGrant::AuthorizationCode)));

    assert!(matches!(&client.auth, Auth::Oauth2 { auth_url, .. } if auth_url.is_empty()));
    assert_eq!(code.auth, oauth2(OAuthGrant::AuthorizationCode));
}
