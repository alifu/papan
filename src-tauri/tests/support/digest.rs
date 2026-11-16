//! Digest routes of the test server (plan 03b §10): `/digest/{algorithm}/{qop}` issues a
//! challenge (after a `Basic` one) and verifies the answer independently of the engine;
//! `/basic` only ever challenges for `Basic` and echoes the `Authorization` it got.

use axum::extract::{Path as UrlPath, Request};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use md5::{Digest as _, Md5};
use serde_json::json;

pub const USER: &str = "Mufasa";
pub const PASSWORD: &str = "Circle of Life";
const REALM: &str = "papan@test";
const NONCE: &str = "7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v";
const OPAQUE: &str = "FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS";

fn hash(algorithm: &str, text: &str) -> String {
    let bytes = if algorithm.starts_with("SHA-256") {
        aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, text.as_bytes())
            .as_ref()
            .to_vec()
    } else {
        Md5::digest(text.as_bytes()).to_vec()
    };
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `key=value` pairs of an `Authorization: Digest …` value (no escapes in test values).
fn params(value: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut rest = value.trim();
    while let Some((key, after)) = rest.split_once('=') {
        let key = key.trim().trim_start_matches(',').trim().to_string();
        let (value, next) = match after.strip_prefix('"') {
            Some(quoted) => {
                let end = quoted.find('"').unwrap_or(quoted.len());
                (&quoted[..end], quoted.get(end + 1..).unwrap_or_default())
            }
            None => {
                let end = after.find(',').unwrap_or(after.len());
                (&after[..end], &after[end..])
            }
        };
        pairs.push((key, value.to_string()));
        rest = next;
    }
    pairs
}

fn verified(algorithm: &str, method: &str, headers: &HeaderMap) -> bool {
    let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Digest "))
    else {
        return false;
    };
    let pairs = params(value);
    let get = |name: &str| {
        pairs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
            .unwrap_or_default()
    };
    let (uri, cnonce, nc) = (get("uri"), get("cnonce"), get("nc"));
    let secret = hash(
        algorithm,
        &format!("{}:{REALM}:{PASSWORD}", get("username")),
    );
    let ha1 = if algorithm.ends_with("-sess") {
        hash(algorithm, &format!("{secret}:{NONCE}:{cnonce}"))
    } else {
        secret
    };
    let ha2 = hash(algorithm, &format!("{method}:{uri}"));
    let expected = if get("qop") == "auth" {
        hash(
            algorithm,
            &format!("{ha1}:{NONCE}:{nc}:{cnonce}:auth:{ha2}"),
        )
    } else {
        hash(algorithm, &format!("{ha1}:{NONCE}:{ha2}"))
    };
    get("username") == USER && get("nonce") == NONCE && get("response") == expected
}

fn challenge(algorithm: &str, qop: &str) -> String {
    let qop = match qop {
        "none" => String::new(),
        other => format!(", qop=\"{other}\""),
    };
    format!(
        "Digest realm=\"{REALM}\", nonce=\"{NONCE}\", algorithm={algorithm}, \
         opaque=\"{OPAQUE}\"{qop}"
    )
}

pub async fn digest(
    UrlPath((algorithm, qop)): UrlPath<(String, String)>,
    request: Request,
) -> Response {
    let method = request.method().to_string();
    let verified = verified(&algorithm, &method, request.headers());
    // Read the body before any reply: answering a challenge with an unread body makes the
    // connection reset on Windows, which fails the client's replay.
    let body = axum::body::to_bytes(request.into_body(), usize::MAX)
        .await
        .unwrap();
    if !verified {
        let mut response = StatusCode::UNAUTHORIZED.into_response();
        let headers = response.headers_mut();
        headers.append(
            header::WWW_AUTHENTICATE,
            format!("Basic realm=\"{REALM}\"").parse().unwrap(),
        );
        headers.append(
            header::WWW_AUTHENTICATE,
            challenge(&algorithm, &qop).parse().unwrap(),
        );
        return response;
    }
    axum::Json(json!({
        "user": USER,
        "body": base64::engine::general_purpose::STANDARD.encode(body),
    }))
    .into_response()
}

pub async fn basic(headers: HeaderMap) -> Response {
    let received = headers
        .get(header::AUTHORIZATION)
        .map(|value| value.to_str().unwrap().to_string())
        .unwrap_or_default();
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, format!("Basic realm=\"{REALM}\""))],
        received,
    )
        .into_response()
}
