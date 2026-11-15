//! Proxy mode (plan 11 §9): a request that matches no route is forwarded to the configured
//! base URL and the answer is relayed unchanged. Redirects are not followed and bodies pass
//! through undecoded, so the client sees what the upstream sent.

use std::time::Duration;

use super::request::RequestCtx;

const TIMEOUT: Duration = Duration::from_secs(30);
/// Marks a request this proxy already forwarded; a mock that sees it again answers 508.
pub const HOPS_HEADER: &str = "x-papan-mock-hops";
pub const MAX_UPSTREAM_BODY: usize = 10 * 1024 * 1024;

/// Hop-by-hop headers (RFC 9110 §7.6.1) plus the ones this hop must set itself.
const SKIPPED: [&str; 12] = [
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "host",
    "x-papan-mock-hops",
    "x-forwarded-for",
];

pub struct Forwarded {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub fn client() -> Result<reqwest::Client, reqwest::Error> {
    client_with_timeout(TIMEOUT)
}

/// The client for outgoing requests of a mock: no redirects, no system proxy, raw bodies.
pub fn client_with_timeout(timeout: Duration) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .build()
}

/// Header names a `Connection` header lists are hop-by-hop too (RFC 9110 §7.6.1).
fn connection_tokens<'a>(headers: impl Iterator<Item = (&'a str, &'a str)>) -> Vec<String> {
    headers
        .filter(|(name, _)| name.eq_ignore_ascii_case("connection"))
        .flat_map(|(_, value)| value.split(','))
        .map(|token| token.trim().to_ascii_lowercase())
        .filter(|token| !token.is_empty())
        .collect()
}

fn skipped(name: &str) -> bool {
    SKIPPED.iter().any(|s| name.eq_ignore_ascii_case(s))
        || name.eq_ignore_ascii_case("content-length")
}

/// The upstream URL for `ctx`: the base, then the request path and query.
pub fn target_url(base: &str, path: &str, query: &str) -> String {
    let mut url = format!("{}{}", base.trim_end_matches('/'), path);
    if !query.is_empty() {
        url.push('?');
        url.push_str(query);
    }
    url
}

pub async fn forward(
    client: &reqwest::Client,
    base: &str,
    ctx: &RequestCtx,
    query: &str,
    body: Vec<u8>,
) -> Result<Forwarded, String> {
    let method = reqwest::Method::from_bytes(ctx.method.as_bytes())
        .map_err(|_| "The request method cannot be forwarded.".to_string())?;
    let mut request = client
        .request(method, target_url(base, &ctx.path, query))
        .body(body);
    let named = connection_tokens(ctx.headers.iter().map(|(n, v)| (n.as_str(), v.as_str())));
    for (name, value) in ctx
        .headers
        .iter()
        .filter(|(n, _)| !skipped(n) && !named.contains(&n.to_ascii_lowercase()))
    {
        request = request.header(name, value);
    }
    let forwarded_for = match ctx.header("x-forwarded-for") {
        Some(earlier) => format!("{earlier}, {}", ctx.ip),
        None => ctx.ip.clone(),
    };
    request = request
        .header("x-forwarded-for", forwarded_for)
        .header(HOPS_HEADER, "1");
    let mut response = request
        .send()
        .await
        .map_err(|_| "The proxy target did not answer.".to_string())?;
    let status = response.status().as_u16();
    let named = connection_tokens(
        response
            .headers()
            .iter()
            .filter_map(|(n, v)| Some((n.as_str(), v.to_str().ok()?))),
    );
    let headers = response
        .headers()
        .iter()
        .filter(|(n, _)| !skipped(n.as_str()) && !named.contains(&n.as_str().to_ascii_lowercase()))
        .filter_map(|(n, v)| Some((n.as_str().to_string(), v.to_str().ok()?.to_string())))
        .collect();
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "The proxy target did not answer.".to_string())?
    {
        if body.len() + chunk.len() > MAX_UPSTREAM_BODY {
            return Err("The proxy target's answer is over 10 MiB.".to_string());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Forwarded {
        status,
        headers,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_base_path_and_query() {
        assert_eq!(
            target_url("https://a.test/v1/", "/users/1", "x=2"),
            "https://a.test/v1/users/1?x=2"
        );
        assert_eq!(target_url("http://a.test", "/p", ""), "http://a.test/p");
    }

    #[test]
    fn hop_by_hop_headers_are_not_forwarded() {
        assert!(skipped("Connection") && skipped("host") && skipped("Content-Length"));
        assert!(!skipped("authorization") && !skipped("x-custom"));
    }
}
