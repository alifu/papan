//! Our URL text → Postman's `url` object. `raw` stays exactly our text; the parts are a
//! best-effort split (URLs may hold `{{vars}}`, so no URL parser), and switched-off params
//! join `query` as `disabled`.

use crate::interop::postman::schema::{QueryParam, Url};
use crate::store::types::Pair;

pub fn url(raw: &str, disabled: &[Pair]) -> Option<Url> {
    if raw.is_empty() && disabled.is_empty() {
        return None;
    }
    let (rest, hash) = split(raw, '#');
    let (rest, query) = split(rest, '?');
    let (protocol, rest) = match rest.split_once("://") {
        Some((scheme, after)) if !scheme.contains('/') => (Some(scheme.to_string()), after),
        _ => (None, rest),
    };
    let (authority, path) = split(rest, '/');
    let (host, port) = host_port(authority);
    let enabled = query.map(query_params).unwrap_or_default();
    let switched_off = disabled.iter().map(|pair| QueryParam {
        key: Some(pair.name.clone()),
        value: Some(pair.value.clone()),
        disabled: true,
        description: None,
    });
    Some(Url {
        raw: Some(raw.to_string()),
        protocol,
        host,
        port,
        path: path
            .map(|p| p.split('/').map(str::to_string).collect())
            .unwrap_or_default(),
        query: enabled.into_iter().chain(switched_off).collect(),
        hash: hash.map(str::to_string),
        variable: Vec::new(),
    })
}

fn split(text: &str, at: char) -> (&str, Option<&str>) {
    match text.split_once(at) {
        Some((before, after)) => (before, Some(after)),
        None => (text, None),
    }
}

fn host_port(authority: &str) -> (Vec<String>, Option<String>) {
    if authority.is_empty() {
        return (Vec::new(), None);
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            (host, Some(port.to_string()))
        }
        _ => (authority, None),
    };
    let parts = if host.contains("{{") {
        vec![host.to_string()]
    } else {
        host.split('.').map(str::to_string).collect()
    };
    (parts, port)
}

fn query_params(query: &str) -> Vec<QueryParam> {
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (key, value) = match part.split_once('=') {
                Some((key, value)) => (key, Some(value.to_string())),
                None => (part, None),
            };
            QueryParam {
                key: Some(key.to_string()),
                value,
                disabled: false,
                description: None,
            }
        })
        .collect()
}
