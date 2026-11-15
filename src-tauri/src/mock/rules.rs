//! Rules and response modes (plan 11 §3).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, PoisonError};

use regex::{Regex, RegexBuilder};
use serde_json::Value;

use super::model::{
    MockResponse, MockRoute, ResponseMode, Rule, RuleOperator, RuleTarget, RulesOperator,
};
use super::request::RequestCtx;

/// Compiled-regex size cap: a user pattern cannot build an enormous automaton.
const REGEX_SIZE_LIMIT: usize = 256 * 1024;

fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

fn actual(rule: &Rule, ctx: &RequestCtx) -> Option<Value> {
    let text = |found: Option<&str>| found.map(|t| Value::String(t.to_string()));
    match rule.target {
        RuleTarget::Body if rule.modifier.trim().is_empty() => {
            (!ctx.body_text.is_empty()).then(|| Value::String(ctx.body_text.clone()))
        }
        RuleTarget::Body => ctx.body_value(&rule.modifier),
        RuleTarget::Query => text(ctx.query_param(&rule.modifier)),
        RuleTarget::Header => text(ctx.header(&rule.modifier)),
        RuleTarget::Params => text(ctx.param(&rule.modifier)),
        RuleTarget::Cookie => text(ctx.cookie(&rule.modifier)),
        RuleTarget::RequestNumber => Some(Value::String(ctx.request_number.to_string())),
        RuleTarget::Path => Some(Value::String(ctx.path.clone())),
    }
}

const REGEX_CACHE_ENTRIES: usize = 256;

type RegexCache = Mutex<HashMap<(String, bool), Option<Regex>>>;

/// Patterns are compiled once and shared by every request (a bad pattern is remembered too).
fn compiled_regex(pattern: &str, ignore_case: bool) -> Option<Regex> {
    static CACHE: OnceLock<RegexCache> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if cache.len() >= REGEX_CACHE_ENTRIES {
        cache.clear();
    }
    cache
        .entry((pattern.to_string(), ignore_case))
        .or_insert_with(|| {
            RegexBuilder::new(pattern)
                .case_insensitive(ignore_case)
                .size_limit(REGEX_SIZE_LIMIT)
                .build()
                .ok()
        })
        .clone()
}

fn regex_matches(pattern: &str, ignore_case: bool, text: &str) -> bool {
    compiled_regex(pattern, ignore_case).is_some_and(|re| re.is_match(text))
}

fn holds(rule: &Rule, ctx: &RequestCtx) -> bool {
    let value = actual(rule, ctx);
    let text = value.as_ref().and_then(text_of);
    let outcome = match rule.operator {
        RuleOperator::Equals => text.as_deref() == Some(rule.value.as_str()),
        RuleOperator::Regex => text
            .as_deref()
            .is_some_and(|t| regex_matches(&rule.value, false, t)),
        RuleOperator::RegexI => text
            .as_deref()
            .is_some_and(|t| regex_matches(&rule.value, true, t)),
        RuleOperator::Null => text.is_none(),
        RuleOperator::EmptyArray => matches!(&value, Some(Value::Array(items)) if items.is_empty()),
        RuleOperator::ArrayIncludes => match &value {
            Some(Value::Array(items)) => items
                .iter()
                .any(|item| text_of(item).as_deref() == Some(rule.value.as_str())),
            _ => false,
        },
    };
    outcome != rule.invert
}

/// A response with no rules never matches by rule (it can still be the default).
fn rules_match(response: &MockResponse, ctx: &RequestCtx) -> bool {
    if response.rules.is_empty() {
        return false;
    }
    match response.rules_operator {
        RulesOperator::And => response.rules.iter().all(|rule| holds(rule, ctx)),
        RulesOperator::Or => response.rules.iter().any(|rule| holds(rule, ctx)),
    }
}

fn default_of(route: &MockRoute) -> &MockResponse {
    route
        .responses
        .iter()
        .find(|r| r.default)
        .unwrap_or(&route.responses[0])
}

/// Picks the response for this request. `random` is a number in `0..len` supplied by the
/// caller so tests are deterministic. The route must have at least one response.
pub fn select<'a>(route: &'a MockRoute, ctx: &RequestCtx, random: usize) -> &'a MockResponse {
    let count = route.responses.len();
    match route.mode {
        ResponseMode::Rules => route
            .responses
            .iter()
            .find(|response| rules_match(response, ctx))
            .unwrap_or_else(|| default_of(route)),
        ResponseMode::Sequential => {
            let turn = usize::try_from(ctx.request_number.saturating_sub(1)).unwrap_or(0);
            &route.responses[turn % count]
        }
        ResponseMode::Random => &route.responses[random % count],
        ResponseMode::DisableRules => default_of(route),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn response(id: &str, rules: Vec<Rule>, default: bool) -> MockResponse {
        MockResponse {
            id: id.into(),
            label: String::new(),
            status: 200,
            headers: vec![],
            body: String::new(),
            latency_ms: 0,
            rules,
            rules_operator: RulesOperator::And,
            default,
            file: String::new(),
            callbacks: vec![],
        }
    }

    fn rule(target: RuleTarget, modifier: &str, operator: RuleOperator, value: &str) -> Rule {
        Rule {
            target,
            modifier: modifier.into(),
            operator,
            value: value.into(),
            invert: false,
        }
    }

    fn route(mode: ResponseMode, responses: Vec<MockResponse>) -> MockRoute {
        MockRoute {
            id: "r".into(),
            method: "get".into(),
            endpoint: "x".into(),
            enabled: true,
            mode,
            kind: super::super::model::RouteKind::Http,
            bucket: String::new(),
            id_key: "id".into(),
            responses,
        }
    }

    fn ctx() -> RequestCtx {
        RequestCtx {
            query: vec![("q".into(), "abc".into())],
            headers: vec![("x-role".into(), "admin".into())],
            cookies: vec![("sid".into(), "7".into())],
            params: vec![("id".into(), "42".into())],
            body_json: Some(json!({"n": 5, "tags": ["a", "b"], "none": null, "empty": []})),
            request_number: 1,
            ..Default::default()
        }
    }

    fn picked(route: &MockRoute, ctx: &RequestCtx) -> String {
        select(route, ctx, 0).id.clone()
    }

    #[test]
    fn rules_mode_picks_first_match_then_default() {
        let r = route(
            ResponseMode::Rules,
            vec![
                response(
                    "admin",
                    vec![rule(
                        RuleTarget::Header,
                        "X-Role",
                        RuleOperator::Equals,
                        "admin",
                    )],
                    false,
                ),
                response("fallback", vec![], true),
            ],
        );
        assert_eq!(picked(&r, &ctx()), "admin");
        let mut other = ctx();
        other.headers.clear();
        assert_eq!(picked(&r, &other), "fallback");
    }

    #[test]
    fn without_a_default_the_first_response_is_used() {
        let r = route(
            ResponseMode::Rules,
            vec![response("a", vec![], false), response("b", vec![], false)],
        );
        assert_eq!(picked(&r, &ctx()), "a");
    }

    #[test]
    fn every_target_and_operator() {
        let c = ctx();
        let check = |target, modifier, operator, value, expected: bool| {
            let r = rule(target, modifier, operator, value);
            assert_eq!(
                holds(&r, &c),
                expected,
                "{target:?} {modifier} {operator:?} {value}"
            );
        };
        check(RuleTarget::Query, "q", RuleOperator::Equals, "abc", true);
        check(RuleTarget::Query, "q", RuleOperator::Regex, "^a.c$", true);
        check(RuleTarget::Query, "q", RuleOperator::Regex, "^A", false);
        check(RuleTarget::Query, "q", RuleOperator::RegexI, "^A", true);
        check(RuleTarget::Query, "missing", RuleOperator::Null, "", true);
        check(RuleTarget::Cookie, "sid", RuleOperator::Equals, "7", true);
        check(RuleTarget::Params, "id", RuleOperator::Equals, "42", true);
        check(RuleTarget::Body, "n", RuleOperator::Equals, "5", true);
        check(
            RuleTarget::Body,
            "tags",
            RuleOperator::ArrayIncludes,
            "b",
            true,
        );
        check(
            RuleTarget::Body,
            "tags",
            RuleOperator::ArrayIncludes,
            "z",
            false,
        );
        check(
            RuleTarget::Body,
            "empty",
            RuleOperator::EmptyArray,
            "",
            true,
        );
        check(
            RuleTarget::Body,
            "tags",
            RuleOperator::EmptyArray,
            "",
            false,
        );
        check(RuleTarget::Body, "none", RuleOperator::Null, "", true);
        check(
            RuleTarget::RequestNumber,
            "",
            RuleOperator::Equals,
            "1",
            true,
        );
        check(RuleTarget::Query, "q", RuleOperator::Regex, "(", false);
    }

    #[test]
    fn invert_and_or_semantics() {
        let mut inverted = rule(RuleTarget::Query, "q", RuleOperator::Equals, "abc");
        inverted.invert = true;
        assert!(!holds(&inverted, &ctx()));

        let mut or = response(
            "or",
            vec![
                rule(RuleTarget::Query, "q", RuleOperator::Equals, "nope"),
                rule(RuleTarget::Query, "q", RuleOperator::Equals, "abc"),
            ],
            false,
        );
        assert!(!rules_match(&or, &ctx()));
        or.rules_operator = RulesOperator::Or;
        assert!(rules_match(&or, &ctx()));
    }

    #[test]
    fn sequential_cycles_and_random_wraps() {
        let r = route(
            ResponseMode::Sequential,
            vec![response("a", vec![], false), response("b", vec![], false)],
        );
        let mut c = ctx();
        let mut seen = Vec::new();
        for n in 1..=3 {
            c.request_number = n;
            seen.push(picked(&r, &c));
        }
        assert_eq!(seen, ["a", "b", "a"]);
        let random = route(ResponseMode::Random, r.responses.clone());
        assert_eq!(select(&random, &c, 3).id, "b");
    }

    #[test]
    fn disable_rules_always_uses_the_default() {
        let r = route(
            ResponseMode::DisableRules,
            vec![
                response(
                    "matches",
                    vec![rule(RuleTarget::Query, "q", RuleOperator::Equals, "abc")],
                    false,
                ),
                response("default", vec![], true),
            ],
        );
        assert_eq!(picked(&r, &ctx()), "default");
    }
}
