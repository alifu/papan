//! Endpoint matching (plan 11 §3): literal segments, `:name` params and `*` wildcards,
//! case-insensitive, one optional trailing slash. The pattern becomes a regex with every
//! literal escaped, so an endpoint can never inject regex syntax.

use regex::Regex;

use super::model::{MockRoute, RouteKind};

#[derive(Debug, Clone)]
pub struct CompiledRoute {
    method: String,
    pattern: Regex,
    params: Vec<String>,
}

/// The matched route's index in the mock and its params.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matched {
    pub index: usize,
    pub params: Vec<(String, String)>,
}

/// Regex source for an endpoint plus its param names; `None` when it cannot compile.
/// The param a CRUD route captures for the item id (`endpoint/<id>`).
pub const ITEM_ID_PARAM: &str = "__id";

fn compile_endpoint(endpoint: &str, crud: bool) -> Option<(Regex, Vec<String>)> {
    let mut source = String::from("(?i)^");
    let mut params = Vec::new();
    let trimmed = endpoint.trim().trim_matches('/');
    for segment in trimmed.split('/').filter(|s| !s.is_empty()) {
        source.push('/');
        if let Some(name) = segment.strip_prefix(':') {
            params.push(name.to_string());
            source.push_str("([^/]+)");
        } else {
            let mut literal = String::new();
            for part in segment.split('*').enumerate().map(|(i, p)| (i > 0, p)) {
                if part.0 {
                    literal.push_str(".*");
                }
                literal.push_str(&regex::escape(part.1));
            }
            source.push_str(&literal);
        }
    }
    if crud {
        source.push_str("(?:/([^/]+))?");
        params.push(ITEM_ID_PARAM.to_string());
    }
    if trimmed.is_empty() {
        source.push('/');
    } else {
        source.push_str("/?");
    }
    source.push('$');
    Regex::new(&source).ok().map(|re| (re, params))
}

impl CompiledRoute {
    pub fn new(route: &MockRoute) -> Option<Self> {
        let crud = route.kind == RouteKind::Crud;
        let (pattern, params) = compile_endpoint(&route.endpoint, crud)?;
        Some(Self {
            method: if crud {
                "any".into()
            } else {
                route.method.clone()
            },
            pattern,
            params,
        })
    }

    fn matches(&self, method: &str, path: &str) -> Option<Vec<(String, String)>> {
        if self.method != "any" && !self.method.eq_ignore_ascii_case(method) {
            return None;
        }
        let captures = self.pattern.captures(path)?;
        Some(
            self.params
                .iter()
                .enumerate()
                .filter_map(|(n, name)| {
                    let raw = captures.get(n + 1)?.as_str();
                    let decoded = percent_decode(raw);
                    Some((name.clone(), decoded))
                })
                .collect(),
        )
    }
}

fn percent_decode(raw: &str) -> String {
    url::form_urlencoded::parse(format!("v={}", raw.replace('+', "%2B")).as_bytes())
        .next()
        .map(|(_, value)| value.into_owned())
        .unwrap_or_else(|| raw.to_string())
}

/// The first enabled route (in file order) matching `method` and the prefix-relative `path`.
pub fn find_route(
    routes: &[MockRoute],
    compiled: &[Option<CompiledRoute>],
    method: &str,
    path: &str,
) -> Option<Matched> {
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    routes
        .iter()
        .zip(compiled)
        .enumerate()
        .filter(|(_, (route, _))| route.enabled)
        .find_map(|(index, (_, compiled))| {
            let params = compiled.as_ref()?.matches(method, &path)?;
            Some(Matched { index, params })
        })
}

/// Strips the mock's prefix from a request path; `None` when the path is outside it.
pub fn strip_prefix<'a>(prefix: &str, path: &'a str) -> Option<&'a str> {
    let prefix = prefix.trim().trim_matches('/');
    if prefix.is_empty() {
        return Some(path);
    }
    let wanted = format!("/{prefix}");
    path.get(..wanted.len())
        .filter(|head| head.eq_ignore_ascii_case(&wanted))?;
    let tail = &path[wanted.len()..];
    (tail.is_empty() || tail.starts_with('/')).then_some(if tail.is_empty() { "/" } else { tail })
}

#[cfg(test)]
mod tests {
    use super::super::model::{MockResponse, ResponseMode, RulesOperator};
    use super::*;

    fn route(method: &str, endpoint: &str, enabled: bool) -> MockRoute {
        MockRoute {
            id: endpoint.into(),
            method: method.into(),
            endpoint: endpoint.into(),
            enabled,
            mode: ResponseMode::Rules,
            kind: super::super::model::RouteKind::Http,
            bucket: String::new(),
            id_key: "id".into(),
            responses: vec![MockResponse {
                id: "r".into(),
                label: String::new(),
                status: 200,
                headers: vec![],
                body: String::new(),
                latency_ms: 0,
                rules: vec![],
                rules_operator: RulesOperator::And,
                default: true,
                file: String::new(),
                callbacks: vec![],
            }],
        }
    }

    fn find(routes: &[MockRoute], method: &str, path: &str) -> Option<Matched> {
        let compiled: Vec<_> = routes.iter().map(CompiledRoute::new).collect();
        find_route(routes, &compiled, method, path)
    }

    #[test]
    fn matches_literals_params_and_trailing_slash() {
        let routes = vec![route("get", "users/:id", true), route("any", "ping", true)];
        let hit = find(&routes, "GET", "/users/42").unwrap();
        assert_eq!(hit.index, 0);
        assert_eq!(hit.params, vec![("id".to_string(), "42".to_string())]);
        assert!(find(&routes, "get", "/users/42/").is_some());
        assert!(find(&routes, "get", "/Users/7").is_some());
        assert!(find(&routes, "post", "/users/42").is_none());
        assert!(find(&routes, "delete", "/ping").is_some());
        assert!(find(&routes, "get", "/users").is_none());
        assert!(find(&routes, "get", "/users/1/extra").is_none());
    }

    #[test]
    fn decodes_params_and_wildcards() {
        let routes = vec![route("get", "files/*", true), route("get", "a/:n", true)];
        assert!(find(&routes, "get", "/files/a/b/c.txt").is_some());
        assert_eq!(
            find(&routes, "get", "/a/x%20y").unwrap().params,
            vec![("n".to_string(), "x y".to_string())]
        );
        assert_eq!(find(&routes, "get", "/a/1%2B1").unwrap().params[0].1, "1+1");
    }

    #[test]
    fn first_enabled_route_wins_and_disabled_are_skipped() {
        let routes = vec![
            route("get", "x", false),
            route("get", ":any", true),
            route("get", "x", true),
        ];
        assert_eq!(find(&routes, "get", "/x").unwrap().index, 1);
    }

    #[test]
    fn regex_characters_in_endpoints_are_literal() {
        let routes = vec![route("get", "a.b(c)", true)];
        assert!(find(&routes, "get", "/a.b(c)").is_some());
        assert!(find(&routes, "get", "/aXb(c)").is_none());
    }

    #[test]
    fn root_route_matches_slash() {
        let routes = vec![route("get", "", true)];
        assert!(find(&routes, "get", "/").is_some());
        assert!(find(&routes, "get", "/x").is_none());
    }

    #[test]
    fn strips_the_prefix_on_segment_boundaries() {
        assert_eq!(strip_prefix("api/v1", "/api/v1/users"), Some("/users"));
        assert_eq!(strip_prefix("/api/v1/", "/API/v1"), Some("/"));
        assert_eq!(strip_prefix("api", "/apix/users"), None);
        assert_eq!(strip_prefix("", "/users"), Some("/users"));
        assert_eq!(strip_prefix("api", "/other"), None);
    }
}
