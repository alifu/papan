//! Mockers environment files, in and out (plan 11 §13). Read leniently from JSON, written as the
//! plain environment object Mockers opens. Things Mockers has and Papan does not are dropped
//! and reported as notes.

use std::collections::HashMap;

use serde_json::{json, Map, Value};

use super::model::{
    default_id_key, DataBucket, MockBind, MockEnvironment, MockResponse, MockRoute, ResponseMode,
    RouteKind, Rule, RuleOperator, RuleTarget, RulesOperator,
};
use crate::error::AppError;
use crate::store::types::KeyValue;

/// The schema version written on export; Mockers migrates older files forward.
const EXPORT_MIGRATION: u32 = 32;

pub struct Converted {
    pub mock: MockEnvironment,
    pub notes: Vec<String>,
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn number(value: &Value, key: &str) -> u32 {
    value
        .get(key)
        .and_then(Value::as_u64)
        .map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX))
}

fn flag(value: &Value, key: &str, default: bool) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn list<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn headers_of(value: &Value) -> Vec<KeyValue> {
    list(value, "headers")
        .iter()
        .filter(|h| !text(h, "key").is_empty())
        .map(|h| KeyValue {
            name: text(h, "key"),
            value: text(h, "value"),
            enabled: true,
        })
        .collect()
}

/// Mockers exports either the environment itself or a wrapper listing its items.
fn environment_of(root: &Value) -> Option<&Value> {
    if root.get("routes").is_some() {
        return Some(root);
    }
    list(root, "data")
        .iter()
        .find(|entry| entry.get("type").and_then(Value::as_str) == Some("environment"))
        .and_then(|entry| entry.get("item"))
}

fn target_of(name: &str) -> Option<RuleTarget> {
    Some(match name {
        "body" => RuleTarget::Body,
        "query" => RuleTarget::Query,
        "header" => RuleTarget::Header,
        "params" => RuleTarget::Params,
        "cookie" => RuleTarget::Cookie,
        "request_number" => RuleTarget::RequestNumber,
        "path" => RuleTarget::Path,
        _ => return None,
    })
}

fn operator_of(name: &str) -> Option<RuleOperator> {
    Some(match name {
        "equals" => RuleOperator::Equals,
        "regex" => RuleOperator::Regex,
        "regex_i" => RuleOperator::RegexI,
        "null" => RuleOperator::Null,
        "empty_array" => RuleOperator::EmptyArray,
        "array_includes" => RuleOperator::ArrayIncludes,
        _ => return None,
    })
}

fn rules_of(response: &Value, label: &str, notes: &mut Vec<String>) -> Vec<Rule> {
    let mut rules = Vec::new();
    for rule in list(response, "rules") {
        match (
            target_of(&text(rule, "target")),
            operator_of(&text(rule, "operator")),
        ) {
            (Some(target), Some(operator)) => rules.push(Rule {
                target,
                modifier: text(rule, "modifier"),
                operator,
                value: text(rule, "value"),
                invert: flag(rule, "invert", false),
            }),
            _ => notes.push(format!(
                "Response \"{label}\": a rule on \"{}\" with \"{}\" is not supported and was left out.",
                text(rule, "target"),
                text(rule, "operator")
            )),
        }
    }
    rules
}

fn response_of(value: &Value, notes: &mut Vec<String>) -> MockResponse {
    let label = text(value, "label");
    let status = value
        .get("statusCode")
        .and_then(Value::as_u64)
        .and_then(|n| u16::try_from(n).ok())
        .unwrap_or(200);
    let file = text(value, "filePath");
    let sends_file = flag(value, "sendFileAsBody", false) && !file.is_empty();
    let usable_file = sends_file && super::model::check_file("import", "file", &file).is_ok();
    if sends_file && !usable_file {
        notes.push(format!(
            "Response \"{label}\": the file path must be inside the collection, so it was left out."
        ));
    }
    if !list(value, "callbacks").is_empty() {
        notes.push(format!("Response \"{label}\": callbacks are not imported."));
    }
    MockResponse {
        id: new_id(),
        rules: rules_of(value, &label, notes),
        label,
        status,
        headers: headers_of(value),
        body: text(value, "body"),
        latency_ms: number(value, "latency"),
        rules_operator: if text(value, "rulesOperator").eq_ignore_ascii_case("or") {
            RulesOperator::Or
        } else {
            RulesOperator::And
        },
        default: flag(value, "default", false),
        file: if usable_file { file } else { String::new() },
        callbacks: Vec::new(),
    }
}

fn mode_of(name: &str) -> ResponseMode {
    match name {
        "SEQUENTIAL" => ResponseMode::Sequential,
        "RANDOM" => ResponseMode::Random,
        "DISABLE_RULES" => ResponseMode::DisableRules,
        _ => ResponseMode::Rules,
    }
}

fn route_of(
    value: &Value,
    buckets: &HashMap<String, String>,
    notes: &mut Vec<String>,
) -> MockRoute {
    let crud = text(value, "type") == "crud";
    let bucket = buckets
        .get(&text(value, "dataBucketID"))
        .cloned()
        .unwrap_or_default();
    let crud = crud && !bucket.is_empty();
    let crud_key = [value]
        .into_iter()
        .chain(list(value, "responses"))
        .map(|v| text(v, "crudKey"))
        .find(|key| !key.is_empty() && key != "id")
        .unwrap_or_default();
    let method = text(value, "method").to_ascii_lowercase();
    let endpoint = text(value, "endpoint").trim_matches('/').to_string();
    let responses = if crud {
        Vec::new()
    } else {
        list(value, "responses")
            .iter()
            .map(|r| response_of(r, notes))
            .collect()
    };
    MockRoute {
        id: new_id(),
        method: if method.is_empty() {
            "get".into()
        } else {
            method
        },
        endpoint,
        enabled: flag(value, "enabled", true),
        mode: mode_of(&text(value, "responseMode")),
        kind: if crud {
            RouteKind::Crud
        } else {
            RouteKind::Http
        },
        bucket: if crud { bucket } else { String::new() },
        id_key: if crud_key.is_empty() {
            default_id_key()
        } else {
            crud_key
        },
        greeting: String::new(),
        responses,
    }
}

/// A bucket's value must be JSON here; Mockers allows templates, which are not kept.
fn buckets_of(env: &Value, notes: &mut Vec<String>) -> (Vec<DataBucket>, HashMap<String, String>) {
    let mut map = HashMap::new();
    let buckets = list(env, "data")
        .iter()
        .map(|b| {
            let id = new_id();
            map.insert(text(b, "uuid"), id.clone());
            let raw = text(b, "value");
            let name = text(b, "name");
            let value = if serde_json::from_str::<Value>(&raw).is_ok() {
                raw
            } else {
                notes.push(format!(
                    "Data bucket \"{name}\" is not plain JSON, so it starts empty."
                ));
                "[]".to_string()
            };
            DataBucket { id, name, value }
        })
        .collect();
    (buckets, map)
}

pub fn import(source: &str) -> Result<Converted, AppError> {
    let root: Value = serde_json::from_str(source).map_err(|_| {
        AppError::invalid_input("That file is not valid JSON, so it is not a Mockers environment.")
    })?;
    let Some(env) = environment_of(&root) else {
        return Err(AppError::invalid_input(
            "That JSON has no Mockers environment in it.",
        ));
    };
    let mut notes = Vec::new();
    let (data, buckets) = buckets_of(env, &mut notes);
    let routes = list(env, "routes")
        .iter()
        .map(|r| route_of(r, &buckets, &mut notes))
        .collect();
    // An imported file never opens the mock to the network or forwards to another host by
    // itself; the settings are left for the user to set on purpose.
    if text(env, "hostname") == "0.0.0.0" {
        notes.push(
            "The mock listens on this computer only; set it to all interfaces to share it.".into(),
        );
    }
    let proxy = text(env, "proxyHost");
    if flag(env, "proxyMode", false) && !proxy.is_empty() {
        notes.push(format!("Forwarding to {proxy} was left off."));
    }
    let name = text(env, "name");
    let mock = MockEnvironment {
        name: if name.trim().is_empty() {
            "Imported mock".into()
        } else {
            name.chars().take(super::model::MAX_NAME_CHARS).collect()
        },
        port: env
            .get("port")
            .and_then(Value::as_u64)
            .and_then(|n| u16::try_from(n).ok())
            .unwrap_or(0),
        bind: MockBind::Loopback,
        prefix: text(env, "endpointPrefix").trim_matches('/').to_string(),
        latency_ms: number(env, "latency"),
        cors: flag(env, "cors", true),
        headers: headers_of(env),
        routes,
        tls: env
            .get("tlsOptions")
            .is_some_and(|t| flag(t, "enabled", false)),
        proxy: String::new(),
        data,
        fallback: None,
    };
    Ok(Converted { mock, notes })
}

fn header_rows(headers: &[KeyValue]) -> Vec<Value> {
    headers
        .iter()
        .filter(|h| h.enabled)
        .map(|h| json!({ "key": h.name, "value": h.value }))
        .collect()
}

fn target_name(target: RuleTarget) -> &'static str {
    match target {
        RuleTarget::Body => "body",
        RuleTarget::Query => "query",
        RuleTarget::Header => "header",
        RuleTarget::Params => "params",
        RuleTarget::Cookie => "cookie",
        RuleTarget::RequestNumber => "request_number",
        RuleTarget::Path => "path",
    }
}

fn operator_name(operator: RuleOperator) -> &'static str {
    match operator {
        RuleOperator::Equals => "equals",
        RuleOperator::Regex => "regex",
        RuleOperator::RegexI => "regex_i",
        RuleOperator::Null => "null",
        RuleOperator::EmptyArray => "empty_array",
        RuleOperator::ArrayIncludes => "array_includes",
    }
}

fn response_json(r: &MockResponse) -> Value {
    json!({
        "uuid": r.id,
        "body": r.body,
        "latency": r.latency_ms,
        "statusCode": r.status,
        "label": r.label,
        "headers": header_rows(&r.headers),
        "bodyType": if r.file.is_empty() { "INLINE" } else { "FILE" },
        "filePath": r.file,
        "databucketID": "",
        "sendFileAsBody": !r.file.is_empty(),
        "rules": r.rules.iter().map(|rule| json!({
            "target": target_name(rule.target),
            "modifier": rule.modifier,
            "value": rule.value,
            "invert": rule.invert,
            "operator": operator_name(rule.operator),
        })).collect::<Vec<_>>(),
        "rulesOperator": if r.rules_operator == RulesOperator::Or { "OR" } else { "AND" },
        "disableTemplating": false,
        "fallbackTo404": false,
        "default": r.default,
        "crudKey": "id",
        "callbacks": [],
    })
}

fn mode_name(mode: ResponseMode) -> Value {
    match mode {
        ResponseMode::Rules => Value::Null,
        ResponseMode::Sequential => json!("SEQUENTIAL"),
        ResponseMode::Random => json!("RANDOM"),
        ResponseMode::DisableRules => json!("DISABLE_RULES"),
    }
}

fn route_json(route: &MockRoute) -> Value {
    let crud = route.kind == RouteKind::Crud;
    let responses: Vec<Value> = if crud {
        // Mockers wants one response on a CRUD route.
        vec![json!({
            "uuid": new_id(), "body": "", "latency": 0, "statusCode": 200, "label": "",
            "headers": [], "bodyType": "DATABUCKET", "filePath": "", "databucketID": "",
            "sendFileAsBody": false, "rules": [], "rulesOperator": "OR",
            "disableTemplating": false, "fallbackTo404": false, "default": true,
            "crudKey": route.id_key, "callbacks": [],
        })]
    } else {
        route.responses.iter().map(response_json).collect()
    };
    json!({
        "uuid": route.id,
        "type": if crud { "crud" } else { "http" },
        "documentation": "",
        "method": if route.method == "any" { "get" } else { route.method.as_str() },
        "endpoint": route.endpoint,
        "responses": responses,
        "enabled": route.enabled,
        "responseMode": mode_name(route.mode),
        "streamingMode": null,
        "streamingInterval": 0,
        "dataBucketID": route.bucket,
    })
}

/// The mock as the environment object Mockers opens.
pub fn export(mock: &MockEnvironment) -> Result<String, AppError> {
    // Mockers files have no WebSocket routes.
    let http_routes: Vec<&MockRoute> = mock
        .routes
        .iter()
        .filter(|r| r.kind != RouteKind::Ws)
        .collect();
    let routes: Vec<Value> = http_routes.iter().map(|r| route_json(r)).collect();
    let children: Vec<Value> = http_routes
        .iter()
        .map(|r| json!({ "type": "route", "uuid": r.id }))
        .collect();
    let data: Vec<Value> = mock
        .data
        .iter()
        .map(|b| json!({ "uuid": b.id, "id": b.id, "name": b.name, "documentation": "", "value": b.value }))
        .collect();
    let mut env = Map::new();
    env.insert("uuid".into(), json!(new_id()));
    env.insert("lastMigration".into(), json!(EXPORT_MIGRATION));
    env.insert("name".into(), json!(mock.name));
    env.insert("endpointPrefix".into(), json!(mock.prefix));
    env.insert("latency".into(), json!(mock.latency_ms));
    env.insert("port".into(), json!(mock.port));
    env.insert(
        "hostname".into(),
        json!(if mock.bind == MockBind::AllInterfaces {
            "0.0.0.0"
        } else {
            "127.0.0.1"
        }),
    );
    env.insert("folders".into(), json!([]));
    env.insert("routes".into(), Value::Array(routes));
    env.insert("rootChildren".into(), Value::Array(children));
    env.insert("proxyMode".into(), json!(!mock.proxy.is_empty()));
    env.insert("proxyHost".into(), json!(mock.proxy));
    env.insert("proxyRemovePrefix".into(), json!(false));
    env.insert(
        "tlsOptions".into(),
        json!({
            "enabled": mock.tls, "type": "CERT", "pfxPath": "", "certPath": "", "keyPath": "",
            "caPath": "", "passphrase": ""
        }),
    );
    env.insert("cors".into(), json!(mock.cors));
    env.insert("headers".into(), Value::Array(header_rows(&mock.headers)));
    env.insert(
        "proxyReqHeaders".into(),
        json!([{ "key": "", "value": "" }]),
    );
    env.insert(
        "proxyResHeaders".into(),
        json!([{ "key": "", "value": "" }]),
    );
    env.insert("data".into(), Value::Array(data));
    env.insert("callbacks".into(), json!([]));
    serde_json::to_string_pretty(&Value::Object(env)).map_err(|source| {
        AppError::internal("Could not write the Mockers file.").with_detail(source.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::model::validate;

    const SAMPLE: &str = r#"{
      "name": "Shop", "port": 3001, "hostname": "0.0.0.0", "endpointPrefix": "/api/",
      "latency": 20, "cors": false, "proxyMode": true, "proxyHost": "https://real.example.com",
      "tlsOptions": {"enabled": true},
      "headers": [{"key": "X-Mock", "value": "1"}],
      "data": [
        {"uuid": "b1", "name": "Users", "value": "[{\"id\":1}]"},
        {"uuid": "b2", "name": "Bad", "value": "{{faker 'x'}}"}
      ],
      "routes": [
        {"uuid": "r1", "method": "POST", "endpoint": "/users/:id", "responseMode": "SEQUENTIAL",
         "responses": [
           {"uuid": "s1", "label": "ok", "statusCode": 201, "body": "{}", "latency": 5,
            "headers": [{"key": "Content-Type", "value": "application/json"}],
            "rulesOperator": "OR", "default": true,
            "rules": [
              {"target": "header", "modifier": "x", "value": "1", "operator": "equals", "invert": true},
              {"target": "bogus", "modifier": "", "value": "", "operator": "equals"}
            ]},
           {"uuid": "s2", "statusCode": 200, "filePath": "/abs/file.json", "sendFileAsBody": true}
         ]},
        {"uuid": "r2", "type": "crud", "endpoint": "people", "dataBucketID": "b1", "crudKey": "pk",
         "responses": [{"uuid": "s3", "statusCode": 200}]}
      ]
    }"#;

    #[test]
    fn a_mockers_environment_becomes_a_valid_mock_with_notes_for_what_is_lost() {
        let done = import(SAMPLE).unwrap();
        let mock = &done.mock;
        validate("test", mock).unwrap();
        assert_eq!(mock.name, "Shop");
        assert_eq!(mock.port, 3001);
        assert_eq!(mock.bind, MockBind::Loopback);
        assert_eq!(mock.prefix, "api");
        assert!(mock.tls && !mock.cors);
        assert!(mock.proxy.is_empty());
        assert_eq!(mock.latency_ms, 20);
        let post = &mock.routes[0];
        assert_eq!(
            (post.method.as_str(), post.endpoint.as_str()),
            ("post", "users/:id")
        );
        assert_eq!(post.mode, ResponseMode::Sequential);
        assert_eq!(post.responses[0].status, 201);
        assert_eq!(post.responses[0].rules.len(), 1);
        assert!(post.responses[0].rules[0].invert);
        assert_eq!(post.responses[0].rules_operator, RulesOperator::Or);
        assert!(post.responses[1].file.is_empty());
        let crud = &mock.routes[1];
        assert_eq!(crud.kind, RouteKind::Crud);
        assert_eq!(crud.id_key, "pk");
        assert_eq!(crud.bucket, mock.data[0].id);
        assert_eq!(mock.data[1].value, "[]");
        assert_eq!(done.notes.len(), 5, "{:?}", done.notes);
    }

    #[test]
    fn the_wrapped_export_format_is_read_too() {
        let wrapped = r#"{"source":"mockers","data":[{"type":"environment","item":{"name":"W","routes":[]}}]}"#;
        assert_eq!(import(wrapped).unwrap().mock.name, "W");
        assert!(import("{}").is_err());
        assert!(import("nope").is_err());
    }

    #[test]
    fn export_then_import_keeps_the_mock() {
        let first = import(SAMPLE).unwrap().mock;
        let text = export(&first).unwrap();
        let back = import(&text).unwrap();
        let (a, b) = (&first, &back.mock);
        assert_eq!(back.notes, Vec::<String>::new());
        assert_eq!(
            (a.name.as_str(), a.port, a.bind),
            (b.name.as_str(), b.port, b.bind)
        );
        assert_eq!(
            (&a.prefix, &a.proxy, a.tls, a.cors),
            (&b.prefix, &b.proxy, b.tls, b.cors)
        );
        assert_eq!(a.routes.len(), b.routes.len());
        assert_eq!(
            a.routes[0].responses[0].rules,
            b.routes[0].responses[0].rules
        );
        assert_eq!(
            a.routes[0].responses[0].headers,
            b.routes[0].responses[0].headers
        );
        assert_eq!(b.routes[1].kind, RouteKind::Crud);
        assert_eq!(b.routes[1].bucket, b.data[0].id);
        assert_eq!(b.routes[1].id_key, "pk");
        assert_eq!(b.data[0].value, a.data[0].value);
    }

    #[test]
    fn export_leaves_out_websocket_routes() {
        let mut mock = import(SAMPLE).unwrap().mock;
        mock.routes[0].kind = RouteKind::Ws;
        let back = import(&export(&mock).unwrap()).unwrap().mock;
        assert_eq!(back.routes.len(), 1);
        assert_eq!(back.routes[0].kind, RouteKind::Crud);
    }
}
