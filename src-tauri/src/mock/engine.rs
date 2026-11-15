//! Turns a parsed request into a reply for one running mock: route match, response
//! selection, templating, CORS and global headers. Pure: no sockets, no clock.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rand::Rng;

use super::matcher::{find_route, strip_prefix, CompiledRoute};
use super::model::{MockEnvironment, MAX_LATENCY_MS};
use super::request::RequestCtx;
use super::rules::select;
use super::template;

/// A mock environment ready to serve: routes compiled, counters per route.
pub struct Compiled {
    pub mock: MockEnvironment,
    routes: Vec<Option<CompiledRoute>>,
    counters: Vec<AtomicU64>,
}

impl Compiled {
    pub fn new(mock: MockEnvironment) -> Self {
        let routes = mock.routes.iter().map(CompiledRoute::new).collect();
        let counters = mock.routes.iter().map(|_| AtomicU64::new(0)).collect();
        Self {
            mock,
            routes,
            counters,
        }
    }
}

/// What to send, plus what the log needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub delay: Duration,
    pub route_id: Option<String>,
    pub response_id: Option<String>,
}

fn set_header(headers: &mut Vec<(String, String)>, name: &str, value: String) {
    headers.retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
    headers.push((name.to_string(), value));
}

fn has_header(headers: &[(String, String)], name: &str) -> bool {
    headers
        .iter()
        .any(|(existing, _)| existing.eq_ignore_ascii_case(name))
}

fn cors_headers(headers: &mut Vec<(String, String)>, ctx: &RequestCtx) {
    set_header(headers, "Access-Control-Allow-Origin", "*".into());
    set_header(
        headers,
        "Access-Control-Allow-Methods",
        "GET,POST,PUT,PATCH,DELETE,HEAD,OPTIONS".into(),
    );
    let allowed = ctx
        .header("access-control-request-headers")
        .unwrap_or("Content-Type, Authorization")
        .to_string();
    set_header(headers, "Access-Control-Allow-Headers", allowed);
}

fn looks_like_json(body: &str) -> bool {
    matches!(body.trim_start().as_bytes().first(), Some(b'{' | b'['))
}

fn error_reply(status: u16, message: &str) -> Reply {
    let body = serde_json::json!({ "error": message }).to_string();
    Reply {
        status,
        headers: vec![("Content-Type".into(), "application/json".into())],
        body,
        delay: Duration::ZERO,
        route_id: None,
        response_id: None,
    }
}

fn capped_delay(milliseconds: u32) -> Duration {
    Duration::from_millis(u64::from(milliseconds.min(MAX_LATENCY_MS)))
}

/// Answers `ctx` (its `path` is the full path; `params` and `request_number` are filled here).
pub fn respond(compiled: &Compiled, mut ctx: RequestCtx) -> Reply {
    let mock = &compiled.mock;
    let Some(relative) = strip_prefix(&mock.prefix, &ctx.path).map(str::to_string) else {
        return with_cors(
            error_reply(404, "No mock route matches this path."),
            mock,
            &ctx,
        );
    };
    let Some(hit) = find_route(&mock.routes, &compiled.routes, &ctx.method, &relative) else {
        if mock.cors && ctx.method.eq_ignore_ascii_case("options") {
            let mut reply = Reply {
                status: 204,
                body: String::new(),
                ..error_reply(204, "")
            };
            reply.headers.clear();
            return with_cors(reply, mock, &ctx);
        }
        let message = format!("No mock route matches {} {}.", ctx.method, ctx.path);
        return with_cors(error_reply(404, &message), mock, &ctx);
    };
    let route = &mock.routes[hit.index];
    ctx.params = hit.params;
    ctx.request_number = compiled.counters[hit.index].fetch_add(1, Ordering::Relaxed) + 1;
    let random = rand::rng().random_range(0..route.responses.len());
    let response = select(route, &ctx, random);

    let mut headers: Vec<(String, String)> = Vec::new();
    for header in mock
        .headers
        .iter()
        .chain(&response.headers)
        .filter(|h| h.enabled)
    {
        set_header(&mut headers, &header.name, header.value.clone());
    }
    let (status, body) = match template::render(&response.body, &ctx) {
        Ok(body) => (response.status, body),
        Err(problem) => {
            tracing::warn!(route = %route.id, "mock template failed: {problem}");
            let reply = error_reply(500, &format!("Template error: {problem}"));
            (reply.status, reply.body)
        }
    };
    if !has_header(&headers, "content-type") {
        let kind = if looks_like_json(&body) {
            "application/json"
        } else {
            "text/plain; charset=utf-8"
        };
        headers.push(("Content-Type".into(), kind.into()));
    }
    let reply = Reply {
        status,
        headers,
        body,
        delay: capped_delay(response.latency_ms.saturating_add(mock.latency_ms)),
        route_id: Some(route.id.clone()),
        response_id: Some(response.id.clone()),
    };
    with_cors(reply, mock, &ctx)
}

fn with_cors(mut reply: Reply, mock: &MockEnvironment, ctx: &RequestCtx) -> Reply {
    if mock.cors {
        cors_headers(&mut reply.headers, ctx);
    }
    reply
}

#[cfg(test)]
mod tests {
    use super::super::model::tests_support::sample;
    use super::super::model::{MockResponse, ResponseMode, Rule, RuleOperator, RuleTarget};
    use super::*;
    use crate::store::types::KeyValue;

    fn request(method: &str, path: &str) -> RequestCtx {
        RequestCtx {
            method: method.into(),
            path: path.into(),
            ..Default::default()
        }
    }

    fn compiled(mock: MockEnvironment) -> Compiled {
        Compiled::new(mock)
    }

    #[test]
    fn serves_a_templated_route_with_params() {
        let mut mock = sample();
        mock.routes[0].responses[0].body = r#"{"id": "{{urlParam 'id'}}"}"#.into();
        let reply = respond(&compiled(mock), request("GET", "/users/9"));
        assert_eq!(reply.status, 200);
        assert_eq!(reply.body, r#"{"id": "9"}"#);
        assert!(has_header(&reply.headers, "content-type"));
        assert_eq!(reply.route_id.as_deref(), Some("r1"));
    }

    #[test]
    fn unmatched_requests_get_a_json_404() {
        let reply = respond(&compiled(sample()), request("GET", "/nope"));
        assert_eq!(reply.status, 404);
        assert!(reply.body.contains("No mock route matches"));
        assert_eq!(reply.route_id, None);
    }

    #[test]
    fn prefix_is_required_and_stripped() {
        let mut mock = sample();
        mock.prefix = "api".into();
        let c = compiled(mock);
        assert_eq!(respond(&c, request("GET", "/api/users/1")).status, 200);
        assert_eq!(respond(&c, request("GET", "/users/1")).status, 404);
    }

    #[test]
    fn cors_headers_and_preflight() {
        let c = compiled(sample());
        let reply = respond(&c, request("GET", "/users/1"));
        assert!(has_header(&reply.headers, "access-control-allow-origin"));
        let preflight = respond(&c, request("OPTIONS", "/users/1"));
        assert_eq!(preflight.status, 204);
        assert!(has_header(
            &preflight.headers,
            "access-control-allow-methods"
        ));
        let mut off = sample();
        off.cors = false;
        let reply = respond(&compiled(off), request("GET", "/users/1"));
        assert!(!has_header(&reply.headers, "access-control-allow-origin"));
    }

    #[test]
    fn response_headers_override_global_ones_and_disabled_are_skipped() {
        let mut mock = sample();
        let header = |name: &str, value: &str, enabled| KeyValue {
            name: name.into(),
            value: value.into(),
            enabled,
        };
        mock.headers = vec![header("X-A", "global", true), header("X-Off", "no", false)];
        mock.routes[0].responses[0].headers = vec![
            header("x-a", "route", true),
            header("Content-Type", "text/csv", true),
        ];
        let reply = respond(&compiled(mock), request("GET", "/users/1"));
        let value = |name: &str| {
            reply
                .headers
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(value("x-a"), Some("route"));
        assert_eq!(value("x-off"), None);
        assert_eq!(value("content-type"), Some("text/csv"));
    }

    #[test]
    fn latency_adds_up_and_is_capped() {
        let mut mock = sample();
        mock.latency_ms = 40;
        mock.routes[0].responses[0].latency_ms = 60;
        assert_eq!(
            respond(&compiled(mock), request("GET", "/users/1")).delay,
            Duration::from_millis(100)
        );
        assert_eq!(capped_delay(u32::MAX), Duration::from_millis(60_000));
    }

    #[test]
    fn sequential_mode_counts_per_route() {
        let mut mock = sample();
        let second = MockResponse {
            id: "s2".into(),
            status: 201,
            default: false,
            ..mock.routes[0].responses[0].clone()
        };
        mock.routes[0].responses.push(second);
        mock.routes[0].mode = ResponseMode::Sequential;
        let c = compiled(mock);
        let statuses: Vec<u16> = (0..3)
            .map(|_| respond(&c, request("GET", "/users/1")).status)
            .collect();
        assert_eq!(statuses, [200, 201, 200]);
    }

    #[test]
    fn rules_pick_the_response_from_the_request() {
        let mut mock = sample();
        let admin = MockResponse {
            id: "admin".into(),
            status: 202,
            default: false,
            rules: vec![Rule {
                target: RuleTarget::Header,
                modifier: "x-role".into(),
                operator: RuleOperator::Equals,
                value: "admin".into(),
                invert: false,
            }],
            ..mock.routes[0].responses[0].clone()
        };
        mock.routes[0].responses.insert(0, admin);
        let c = compiled(mock);
        let mut with_header = request("GET", "/users/1");
        with_header.headers = vec![("x-role".into(), "admin".into())];
        assert_eq!(respond(&c, with_header).status, 202);
        assert_eq!(respond(&c, request("GET", "/users/1")).status, 200);
    }

    #[test]
    fn a_broken_template_is_a_500_not_a_crash() {
        let mut mock = sample();
        mock.routes[0].responses[0].body = "{{divide 1 0}}".into();
        let reply = respond(&compiled(mock), request("GET", "/users/1"));
        assert_eq!(reply.status, 500);
        assert!(reply.body.contains("Template error"));
    }
}
