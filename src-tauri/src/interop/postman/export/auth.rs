//! Our request auth → Postman v2.1 auth (plan 09 §3 auth table). Values come from the request
//! file as stored (literal secrets are already `{{refs}}` there), parameters in key order.

use serde_json::Value;

use super::Report;
use crate::interop::postman::schema::{Auth as PmAuth, AuthParam};
use crate::interop::postman::types::ReportLevel;
use crate::store::types::{ApiKeyPlacement, Auth, ClientAuth, OAuthGrant};

/// `noauth` for no auth: we have no inheritance, so Postman must not inherit either.
pub fn auth(auth: &Auth, path: &str, report: &mut Report) -> Option<PmAuth> {
    let (kind, params): (&str, Vec<(&str, Value)>) = match auth {
        Auth::None => ("noauth", Vec::new()),
        Auth::Unsupported => {
            report.add(
                ReportLevel::Skipped,
                path,
                "Auth made by a newer Papan was not exported.",
            );
            return None;
        }
        Auth::Basic { username, password } => ("basic", user_pass(username, password)),
        Auth::Digest { username, password } => ("digest", user_pass(username, password)),
        Auth::Bearer { token } => ("bearer", vec![("token", text(token))]),
        Auth::ApiKey {
            name,
            value,
            placement,
        } => {
            let place = match placement {
                ApiKeyPlacement::Header => "header",
                ApiKeyPlacement::Query => "query",
            };
            (
                "apikey",
                vec![
                    ("in", text(place)),
                    ("key", text(name)),
                    ("value", text(value)),
                ],
            )
        }
        Auth::Oauth2 {
            grant,
            auth_url,
            token_url,
            client_id,
            client_secret,
            scope,
            client_auth,
            ..
        } => {
            let grant_type = match grant {
                OAuthGrant::ClientCredentials => "client_credentials",
                OAuthGrant::AuthorizationCode => "authorization_code",
            };
            let client_authentication = match client_auth {
                ClientAuth::Basic => "header",
                ClientAuth::Body => "body",
            };
            let code = matches!(grant, OAuthGrant::AuthorizationCode);
            let params = [
                ("accessTokenUrl", text(token_url)),
                ("addTokenTo", text("header")),
                ("authUrl", text(auth_url)),
                ("client_authentication", text(client_authentication)),
                ("clientId", text(client_id)),
                ("clientSecret", text(client_secret)),
                ("grant_type", text(grant_type)),
                ("scope", text(scope)),
            ]
            .into_iter()
            .filter(|(key, _)| code || *key != "authUrl")
            .collect();
            ("oauth2", params)
        }
    };
    let params = params
        .into_iter()
        .map(|(key, value)| AuthParam {
            key: key.to_string(),
            value,
            kind: Some("string".into()),
        })
        .collect();
    Some(PmAuth {
        kind: kind.to_string(),
        params,
    })
}

fn text(s: &str) -> Value {
    Value::String(s.to_string())
}

fn user_pass(username: &str, password: &str) -> Vec<(&'static str, Value)> {
    vec![("password", text(password)), ("username", text(username))]
}
