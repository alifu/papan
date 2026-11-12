//! Redirects are followed here, not by reqwest, so that every secret header is dropped
//! when the origin changes (plan 03 §6, security review): `Authorization`,
//! `Proxy-Authorization`, `Cookie`, any sensitive header name and the API key header.

use reqwest::header::{
    HeaderName, HeaderValue, AUTHORIZATION, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, COOKIE,
    PROXY_AUTHORIZATION, TRANSFER_ENCODING,
};
use reqwest::{Method, StatusCode};
use url::Url;

use super::build::{Prepared, PreparedBody};
use crate::logging::redact::is_sensitive_header;

pub const MAX_REDIRECTS: usize = 10;

fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

fn is_secret(name: &HeaderName, prepared: &Prepared) -> bool {
    [AUTHORIZATION, PROXY_AUTHORIZATION, COOKIE].contains(name)
        || is_sensitive_header(name.as_str())
        || prepared.secret_headers.contains(name)
}

fn is_body_header(name: &HeaderName) -> bool {
    [
        CONTENT_TYPE,
        CONTENT_LENGTH,
        CONTENT_ENCODING,
        TRANSFER_ENCODING,
    ]
    .contains(name)
}

/// The request to send after a `status` response with `location`, or `None` when the
/// response is final (not a redirect, no usable `Location`, or a non-http(s) target).
pub fn next_hop(
    current: &Prepared,
    status: StatusCode,
    location: Option<&HeaderValue>,
) -> Option<Prepared> {
    let redirect = [301, 302, 303, 307, 308].contains(&status.as_u16());
    let target = location
        .filter(|_| redirect)
        .and_then(|value| value.to_str().ok())
        .and_then(|text| current.url.join(text).ok())
        .filter(|url| ["http", "https"].contains(&url.scheme()))?;
    let becomes_get = match status.as_u16() {
        303 => current.method != Method::HEAD,
        301 | 302 => current.method == Method::POST,
        _ => false,
    };
    let cross_origin = !same_origin(&current.url, &target);
    let headers = current
        .headers
        .iter()
        .filter(|(name, _)| !(becomes_get && is_body_header(name)))
        .filter(|(name, _)| !(cross_origin && is_secret(name, current)))
        .cloned()
        .collect();
    Some(Prepared {
        method: if becomes_get {
            Method::GET
        } else {
            current.method.clone()
        },
        url: target,
        headers,
        body: if becomes_get {
            PreparedBody::None
        } else {
            current.body.clone()
        },
        secret_headers: current.secret_headers.clone(),
        // The Digest password is a secret like any other: never offered to a new origin.
        digest: current.digest.clone().filter(|_| !cross_origin),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::http::digest::DigestCredentials;

    fn prepared(method: Method, url: &str) -> Prepared {
        let header = |name: &'static str, value: &'static str| {
            (
                HeaderName::from_static(name),
                HeaderValue::from_static(value),
            )
        };
        Prepared {
            method,
            url: Url::parse(url).unwrap(),
            headers: vec![
                header("authorization", "Bearer t"),
                header("x-api-key", "typed"),
                header("x-custom-key", "k"),
                header("cookie", "a=1"),
                header("content-type", "application/json"),
                header("accept", "*/*"),
            ],
            body: PreparedBody::Bytes(b"{}".to_vec()),
            secret_headers: vec![HeaderName::from_static("x-custom-key")],
            digest: Some(DigestCredentials {
                username: "u".into(),
                password: "p".into(),
            }),
        }
    }

    fn names(prepared: &Prepared) -> Vec<&str> {
        prepared
            .headers
            .iter()
            .map(|(name, _)| name.as_str())
            .collect()
    }

    fn hop(current: &Prepared, status: u16, location: &'static str) -> Option<Prepared> {
        next_hop(
            current,
            StatusCode::from_u16(status).unwrap(),
            Some(&HeaderValue::from_static(location)),
        )
    }

    #[test]
    fn a_cross_origin_redirect_drops_every_secret_header() {
        let current = prepared(Method::GET, "https://a.test/x");

        for location in [
            "https://b.test/y",
            "http://a.test/y",
            "https://a.test:444/y",
        ] {
            let next = hop(&current, 307, location).unwrap();
            assert_eq!(names(&next), ["content-type", "accept"], "{location}");
            assert_eq!(next.digest, None, "{location}");
        }
    }

    #[test]
    fn a_same_origin_redirect_keeps_headers_and_resolves_relative_locations() {
        let current = prepared(Method::PUT, "https://a.test:443/x/y");

        let next = hop(&current, 308, "z?q=1").unwrap();

        assert_eq!(next.url.as_str(), "https://a.test/x/z?q=1");
        assert_eq!(next.headers.len(), 6);
        assert_eq!(next.digest, current.digest);
        assert_eq!((next.method, next.body), (Method::PUT, current.body));
    }

    #[test]
    fn see_other_and_post_on_301_302_become_get_without_a_body() {
        let post = prepared(Method::POST, "https://a.test/x");
        let put = prepared(Method::PUT, "https://a.test/x");
        let head = prepared(Method::HEAD, "https://a.test/x");

        for (current, status, method, has_body) in [
            (&post, 301, Method::GET, false),
            (&post, 302, Method::GET, false),
            (&put, 303, Method::GET, false),
            (&put, 302, Method::PUT, true),
            (&head, 303, Method::HEAD, true),
            (&post, 307, Method::POST, true),
        ] {
            let next = hop(current, status, "/y").unwrap();
            assert_eq!(next.method, method, "{status}");
            assert_eq!(next.body != PreparedBody::None, has_body, "{status}");
            assert_eq!(names(&next).contains(&"content-type"), has_body, "{status}");
        }
    }

    #[test]
    fn non_redirects_and_unusable_locations_are_final() {
        let current = prepared(Method::GET, "https://a.test/x");

        assert!(hop(&current, 200, "/y").is_none());
        assert!(hop(&current, 304, "/y").is_none());
        assert!(hop(&current, 302, "file:///etc/passwd").is_none());
        assert!(next_hop(&current, StatusCode::FOUND, None).is_none());
    }
}
