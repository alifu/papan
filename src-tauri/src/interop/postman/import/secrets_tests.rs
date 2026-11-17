use super::*;
use crate::store::types::ApiKeyPlacement;

fn header(name: &str, value: &str) -> KeyValue {
    KeyValue {
        name: name.into(),
        value: value.into(),
        enabled: true,
    }
}

fn content(auth: Auth, headers: Vec<KeyValue>) -> RequestContent {
    RequestContent {
        auth,
        headers,
        ..RequestContent::new("GET", "https://x")
    }
}

#[test]
fn debug_never_prints_values() {
    let mut values = SecretValues::default();
    values.insert("token", "hunter2");

    let printed = format!("{values:?}");

    assert!(printed.contains("token"));
    assert!(!printed.contains("hunter2"));
}

#[test]
fn literal_credentials_become_secret_variables_and_refs_stay() {
    let mut lifter = Lifter::new(Vec::new(), 10);
    let input = content(
        Auth::Basic {
            username: "u".into(),
            password: "p@ss".into(),
        },
        vec![
            header("Authorization", "Bearer abc"),
            header("X-Api-Key", "{{key}}"),
            header("Accept", "json"),
        ],
    );

    let (lifted, notes) = lifter.lift("get-user", &input);

    assert_eq!(
        lifted.auth,
        Auth::Basic {
            username: "u".into(),
            password: "{{get_user_password}}".into()
        }
    );
    assert_eq!(lifted.headers[0].value, "{{get_user_authorization}}");
    assert_eq!(lifted.headers[1].value, "{{key}}");
    assert_eq!(lifted.headers[2].value, "json");
    assert_eq!(notes.len(), 2);
    assert!(notes
        .iter()
        .all(|note| !note.contains("p@ss") && !note.contains("abc")));
    let values = lifter.into_values();
    assert_eq!(values.get("get_user_password"), Some("p@ss"));
    assert_eq!(values.get("get_user_authorization"), Some("Bearer abc"));
}

#[test]
fn equal_literals_share_one_variable_and_names_never_clash() {
    let mut lifter = Lifter::new(vec!["a_token".to_string()], 10);
    let bearer = |token: &str| {
        content(
            Auth::Bearer {
                token: token.into(),
            },
            Vec::new(),
        )
    };

    let (first, _) = lifter.lift("a", &bearer("t1"));
    let (second, _) = lifter.lift("b", &bearer("t1"));
    let (third, _) = lifter.lift("a", &bearer("t2"));

    assert_eq!(
        first.auth,
        Auth::Bearer {
            token: "{{a_token_2}}".into()
        }
    );
    assert_eq!(second.auth, first.auth);
    assert_eq!(
        third.auth,
        Auth::Bearer {
            token: "{{a_token_3}}".into()
        }
    );
    assert_eq!(lifter.into_values().len(), 2);
}

#[test]
fn api_key_digest_and_oauth_secrets_are_lifted() {
    let mut lifter = Lifter::new(Vec::new(), 10);
    let api = content(
        Auth::ApiKey {
            name: "k".into(),
            value: "v".into(),
            placement: ApiKeyPlacement::Header,
        },
        Vec::new(),
    );
    let oauth = content(
        Auth::Oauth2 {
            grant: crate::store::types::OAuthGrant::ClientCredentials,
            auth_url: String::new(),
            token_url: "https://t".into(),
            client_id: "id".into(),
            client_secret: "cs".into(),
            scope: String::new(),
            client_auth: crate::store::types::ClientAuth::Basic,
            redirect_port: 0,
        },
        Vec::new(),
    );

    let (api, _) = lifter.lift("r", &api);
    let (oauth, _) = lifter.lift("r", &oauth);

    assert!(matches!(api.auth, Auth::ApiKey { ref value, .. } if value == "{{r_api_key}}"));
    assert!(
        matches!(oauth.auth, Auth::Oauth2 { ref client_secret, ref client_id, .. }
            if client_secret == "{{r_client_secret}}" && client_id == "id")
    );
}

#[test]
fn without_room_the_literal_is_removed_not_kept() {
    let mut lifter = Lifter::new(Vec::new(), 0);
    let input = content(Auth::Bearer { token: "t".into() }, Vec::new());

    let (lifted, notes) = lifter.lift("r", &input);

    assert_eq!(
        lifted.auth,
        Auth::Bearer {
            token: String::new()
        }
    );
    assert!(notes[0].contains("removed"));
}

#[test]
fn sensitive_names() {
    for name in [
        "token",
        "API_KEY",
        "apiKey",
        "clientSecret",
        "password",
        "authHeader",
    ] {
        assert!(looks_sensitive(name), "{name}");
    }
    for name in ["baseUrl", "user", "id"] {
        assert!(!looks_sensitive(name), "{name}");
    }
}
