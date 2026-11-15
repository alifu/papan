//! "Mock from collection" (plan 11 §12): one route per HTTP-like request of a collection,
//! so a front end can be built against the API before the API exists.

use super::model::{
    default_id_key, MockBind, MockEnvironment, MockResponse, MockRoute, ResponseMode, RouteKind,
    RulesOperator, MAX_NAME_CHARS, MAX_ROUTES,
};

/// What a request contributes: its name and where it points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub name: String,
    pub method: String,
    pub url: String,
}

const MAX_ENDPOINT_CHARS: usize = 1_000;
const KNOWN_METHODS: [&str; 7] = ["get", "post", "put", "patch", "delete", "head", "options"];

/// The part of a variable name that can stand as a route param.
fn param_name(raw: &str) -> String {
    let name: String = raw
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() {
        "param".to_string()
    } else {
        name
    }
}

/// Replaces every `{{...}}` in a segment with `*`.
fn wildcard_variables(segment: &str) -> String {
    let mut out = String::new();
    let mut rest = segment;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        match rest[start..].find("}}") {
            Some(end) => {
                out.push('*');
                rest = &rest[start + end + 2..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn segment_of(segment: &str) -> String {
    let trimmed = segment.trim();
    if let Some(inner) = trimmed
        .strip_prefix("{{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .filter(|inner| !inner.contains("{{"))
    {
        return format!(":{}", param_name(inner));
    }
    if trimmed.contains("{{") {
        return wildcard_variables(trimmed);
    }
    trimmed.to_string()
}

/// The route endpoint for a request URL: no scheme, host, base variable, query or fragment;
/// whole-segment variables become `:params`.
pub fn endpoint_of(url: &str) -> String {
    let url = url.trim();
    let without_query = url.split(['?', '#']).next().unwrap_or("");
    let path = match without_query.find("://") {
        Some(at)
            if without_query[..at]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c)) =>
        {
            let after = &without_query[at + 3..];
            after.find('/').map_or("", |slash| &after[slash..])
        }
        _ => match without_query
            .strip_prefix("{{")
            .and_then(|rest| rest.find("}}"))
        {
            // A leading variable that is the whole host part (`{{baseUrl}}/users`).
            Some(end) => {
                let rest = &without_query[end + 4..];
                if rest.is_empty() || rest.starts_with('/') {
                    rest
                } else {
                    without_query
                }
            }
            None => without_query,
        },
    };
    path.split('/')
        .filter(|segment| !segment.trim().is_empty())
        .map(segment_of)
        .collect::<Vec<_>>()
        .join("/")
}

fn method_of(method: &str) -> String {
    let lower = method.trim().to_ascii_lowercase();
    if KNOWN_METHODS.contains(&lower.as_str()) {
        lower
    } else {
        "any".to_string()
    }
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn route_for(source: &Source, method: String, endpoint: String) -> MockRoute {
    MockRoute {
        id: new_id(),
        method,
        endpoint,
        enabled: true,
        mode: ResponseMode::Rules,
        kind: RouteKind::Http,
        bucket: String::new(),
        id_key: default_id_key(),
        responses: vec![MockResponse {
            id: new_id(),
            label: source.name.chars().take(MAX_NAME_CHARS).collect(),
            status: 200,
            headers: vec![],
            body: "{}".to_string(),
            latency_ms: 0,
            rules: vec![],
            rules_operator: RulesOperator::And,
            default: true,
            file: String::new(),
            callbacks: vec![],
        }],
    }
}

/// A mock with one route per distinct method and endpoint, in the order of `sources`.
pub fn mock_from(collection_name: &str, sources: &[Source]) -> MockEnvironment {
    let mut routes: Vec<MockRoute> = Vec::new();
    for source in sources {
        let endpoint = endpoint_of(&source.url);
        let method = method_of(&source.method);
        let repeated = routes
            .iter()
            .any(|r| r.method == method && r.endpoint.eq_ignore_ascii_case(&endpoint));
        if repeated || endpoint.chars().count() > MAX_ENDPOINT_CHARS {
            continue;
        }
        routes.push(route_for(source, method, endpoint));
        if routes.len() >= MAX_ROUTES {
            break;
        }
    }
    let name: String = format!("{collection_name} mock")
        .chars()
        .take(MAX_NAME_CHARS)
        .collect();
    MockEnvironment {
        name,
        port: 0,
        bind: MockBind::Loopback,
        prefix: String::new(),
        latency_ms: 0,
        cors: true,
        headers: vec![],
        routes,
        tls: false,
        proxy: String::new(),
        data: vec![],
        fallback: None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::validate;
    use super::*;

    fn source(name: &str, method: &str, url: &str) -> Source {
        Source {
            name: name.into(),
            method: method.into(),
            url: url.into(),
        }
    }

    #[test]
    fn endpoints_lose_scheme_host_base_variable_and_query() {
        let cases = [
            ("https://api.example.com/v1/users?page=2#top", "v1/users"),
            ("http://localhost:3000/", ""),
            ("{{baseUrl}}/users/list", "users/list"),
            ("{{baseUrl}}", ""),
            ("/users", "users"),
            ("users/all/", "users/all"),
            ("https://host", ""),
        ];
        for (url, expected) in cases {
            assert_eq!(endpoint_of(url), expected, "{url}");
        }
    }

    #[test]
    fn variables_in_the_path_become_params_or_wildcards() {
        assert_eq!(
            endpoint_of("{{base}}/users/{{id}}/posts/{{post id}}"),
            "users/:id/posts/:post_id"
        );
        assert_eq!(endpoint_of("{{base}}/files/{{name}}.json"), "files/*.json");
        assert_eq!(endpoint_of("https://x.test/a/:id"), "a/:id");
        assert_eq!(endpoint_of("{{base}}/{{}}"), ":param");
        assert_eq!(endpoint_of("{{base}}/{{ }}/x"), ":param/x");
    }

    #[test]
    fn one_route_per_distinct_method_and_endpoint() {
        let mock = mock_from(
            "Shop",
            &[
                source("List", "GET", "{{base}}/items"),
                source("Again", "get", "https://x.test/items?x=1"),
                source("Create", "POST", "{{base}}/items"),
                source("Weird", "PROPFIND", "{{base}}/dav"),
            ],
        );
        let routes: Vec<_> = mock
            .routes
            .iter()
            .map(|r| (r.method.as_str(), r.endpoint.as_str()))
            .collect();
        assert_eq!(
            routes,
            [("get", "items"), ("post", "items"), ("any", "dav")]
        );
        assert_eq!(mock.name, "Shop mock");
        assert_eq!(mock.routes[0].responses[0].label, "List");
        assert!(mock.routes[0].responses[0].default);
        assert!(validate("f", &mock).is_ok());
    }

    #[test]
    fn long_names_and_many_requests_stay_within_the_limits() {
        let sources: Vec<Source> = (0..600)
            .map(|n| source(&"n".repeat(300), "GET", &format!("{{{{b}}}}/r{n}")))
            .collect();
        let mock = mock_from(&"c".repeat(300), &sources);
        assert_eq!(mock.routes.len(), MAX_ROUTES);
        assert!(validate("f", &mock).is_ok());
        let long = source("x", "GET", &format!("{{{{b}}}}/{}", "a".repeat(2_000)));
        assert!(mock_from("c", &[long]).routes.is_empty());
    }
}
