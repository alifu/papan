//! OpenAPI 3 / Swagger 2 documents become a mock: one route per path and method, one response
//! per documented status, bodies from examples or built from the schema (plan 11 §13).

use serde_json::{json, Map, Value};

use super::mockers::Converted;
use super::model::{
    default_id_key, MockEnvironment, MockResponse, MockRoute, ResponseMode, RouteKind,
    RulesOperator, MAX_RESPONSES, MAX_ROUTES,
};
use crate::error::AppError;
use crate::store::types::KeyValue;

const METHODS: [&str; 7] = ["get", "post", "put", "patch", "delete", "head", "options"];
const MAX_SCHEMA_DEPTH: usize = 8;
const MAX_NODES: usize = 2_000;
const FALLBACK_STATUS: u16 = 200;

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Reads JSON, or YAML when it is not JSON.
fn parse(source: &str) -> Result<Value, AppError> {
    if let Ok(value) = serde_json::from_str::<Value>(source) {
        return Ok(value);
    }
    crate::store::yaml::parse::<Value>("the OpenAPI file", source.as_bytes())
        .map_err(|_| AppError::invalid_input("That file is neither valid JSON nor valid YAML."))
}

/// Follows a local `$ref` such as `#/components/schemas/Pet`; anything else is not followed.
fn resolve<'a>(root: &'a Value, node: &'a Value) -> &'a Value {
    let mut current = node;
    for _ in 0..MAX_SCHEMA_DEPTH {
        let Some(target) = current.get("$ref").and_then(Value::as_str) else {
            return current;
        };
        let Some(pointer) = target.strip_prefix('#') else {
            return current;
        };
        match root.pointer(pointer) {
            Some(next) => current = next,
            None => return current,
        }
    }
    current
}

/// A sample value for a schema: its example or default, else a value of its type.
fn sample(root: &Value, schema: &Value, depth: usize, budget: &mut usize) -> Value {
    *budget = budget.saturating_sub(1);
    if depth > MAX_SCHEMA_DEPTH || *budget == 0 {
        return Value::Null;
    }
    let schema = resolve(root, schema);
    for key in ["example", "default"] {
        if let Some(value) = schema.get(key) {
            return value.clone();
        }
    }
    if let Some(first) = schema
        .get("enum")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
    {
        return first.clone();
    }
    if let Some(parts) = schema.get("allOf").and_then(Value::as_array) {
        let mut merged = Map::new();
        for part in parts {
            if let Value::Object(fields) = sample(root, part, depth + 1, budget) {
                merged.extend(fields);
            }
        }
        return Value::Object(merged);
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(first) = schema
            .get(key)
            .and_then(Value::as_array)
            .and_then(|a| a.first())
        {
            return sample(root, first, depth + 1, budget);
        }
    }
    let kind = schema
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            if schema.get("properties").is_some() {
                "object"
            } else {
                ""
            }
        });
    match kind {
        "object" => {
            let fields = schema
                .get("properties")
                .and_then(Value::as_object)
                .map(|props| {
                    props
                        .iter()
                        .map(|(name, prop)| (name.clone(), sample(root, prop, depth + 1, budget)))
                        .collect()
                })
                .unwrap_or_default();
            Value::Object(fields)
        }
        "array" => match schema.get("items") {
            Some(items) => json!([sample(root, items, depth + 1, budget)]),
            None => json!([]),
        },
        "integer" | "number" => json!(0),
        "boolean" => json!(true),
        "string" => json!(match schema.get("format").and_then(Value::as_str) {
            Some("date-time") => "2024-01-01T00:00:00Z",
            Some("date") => "2024-01-01",
            Some("uuid") => "00000000-0000-4000-8000-000000000000",
            Some("email") => "user@example.com",
            Some("uri" | "url") => "https://example.com",
            _ => "string",
        }),
        _ => Value::Null,
    }
}

/// An example attached to a media type: `example`, then the first of `examples`.
fn declared_example(root: &Value, media: &Value) -> Option<Value> {
    if let Some(value) = media.get("example") {
        return Some(value.clone());
    }
    let first = media.get("examples")?.as_object()?.values().next()?;
    let first = resolve(root, first);
    first.get("value").cloned()
}

/// The body for one response: (content type, body text).
fn body_of(root: &Value, response: &Value, budget: &mut usize) -> (String, String) {
    // OpenAPI 3: content by media type, JSON preferred.
    if let Some(content) = response.get("content").and_then(Value::as_object) {
        let pick = content
            .iter()
            .find(|(mime, _)| mime.contains("json"))
            .or_else(|| content.iter().next());
        if let Some((mime, media)) = pick {
            let value = declared_example(root, media).or_else(|| {
                media
                    .get("schema")
                    .map(|schema| sample(root, schema, 0, budget))
            });
            return (mime.clone(), render(value));
        }
    }
    // Swagger 2: `examples` by media type and a bare `schema`.
    if let Some((mime, value)) = response
        .get("examples")
        .and_then(Value::as_object)
        .and_then(|e| e.iter().next())
    {
        return (mime.clone(), render(Some(value.clone())));
    }
    match response.get("schema") {
        Some(schema) => (
            "application/json".into(),
            render(Some(sample(root, schema, 0, budget))),
        ),
        None => (String::new(), String::new()),
    }
}

fn render(value: Option<Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text,
        Some(other) => serde_json::to_string_pretty(&other).unwrap_or_default(),
    }
}

/// Status codes in ascending order; `default` and ranges such as `2XX` become a plain status.
fn status_of(code: &str) -> u16 {
    match code.parse::<u16>() {
        Ok(status) if (100..=599).contains(&status) => status,
        _ => match code.chars().next() {
            Some(first @ '1'..='5') if code.len() == 3 => u16::from(first as u8 - b'0') * 100,
            _ => FALLBACK_STATUS,
        },
    }
}

fn responses_of(root: &Value, operation: &Value, budget: &mut usize) -> Vec<MockResponse> {
    let mut found: Vec<(u16, String, MockResponse)> = Vec::new();
    for (code, raw) in operation
        .get("responses")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let response = resolve(root, raw);
        let status = status_of(code);
        let (mime, body) = body_of(root, response, budget);
        let description = response
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default();
        found.push((
            status,
            code.clone(),
            MockResponse {
                id: new_id(),
                label: format!("{code} {description}")
                    .trim()
                    .chars()
                    .take(100)
                    .collect(),
                status,
                headers: if mime.is_empty() {
                    Vec::new()
                } else {
                    vec![KeyValue {
                        name: "Content-Type".into(),
                        value: mime,
                        enabled: true,
                    }]
                },
                body,
                latency_ms: 0,
                rules: Vec::new(),
                rules_operator: RulesOperator::And,
                default: false,
                file: String::new(),
                callbacks: Vec::new(),
            },
        ));
    }
    found.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    found.truncate(MAX_RESPONSES);
    let mut responses: Vec<MockResponse> = found.into_iter().map(|(_, _, r)| r).collect();
    // Answer with the first success; with none documented, a bare 200.
    let first_ok = responses
        .iter()
        .position(|r| (200..300).contains(&r.status));
    match first_ok {
        Some(at) => {
            let chosen = responses.remove(at);
            responses.insert(0, chosen);
        }
        None if responses.is_empty() => responses.push(MockResponse {
            id: new_id(),
            label: "200".into(),
            status: FALLBACK_STATUS,
            headers: Vec::new(),
            body: String::new(),
            latency_ms: 0,
            rules: Vec::new(),
            rules_operator: RulesOperator::And,
            default: false,
            file: String::new(),
            callbacks: Vec::new(),
        }),
        None => {}
    }
    responses[0].default = true;
    responses
}

/// `/pets/{id}` becomes `pets/:id`.
fn endpoint_of(path: &str) -> String {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(|segment| match segment.strip_prefix('{') {
            Some(name) if segment.ends_with('}') && segment.matches('{').count() == 1 => {
                format!(":{}", &name[..name.len() - 1])
            }
            _ if segment.contains('{') => "*".to_string(),
            _ => segment.to_string(),
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// The path part of the first server (`https://x.test/v1` gives `v1`), or Swagger 2 `basePath`.
fn prefix_of(root: &Value) -> String {
    let url = root
        .pointer("/servers/0/url")
        .and_then(Value::as_str)
        .or_else(|| root.get("basePath").and_then(Value::as_str))
        .unwrap_or_default();
    let path = match url.split_once("://") {
        Some((_, rest)) => rest.split_once('/').map_or("", |(_, path)| path),
        None => url,
    };
    path.split('/')
        .filter(|s| !s.is_empty() && !s.contains('{'))
        .collect::<Vec<_>>()
        .join("/")
}

pub fn import(source: &str) -> Result<Converted, AppError> {
    let root = parse(source)?;
    let Some(paths) = root.get("paths").and_then(Value::as_object) else {
        return Err(AppError::invalid_input(
            "That file has no \"paths\", so it is not an OpenAPI document.",
        ));
    };
    let mut notes = Vec::new();
    let mut budget = MAX_NODES * 20;
    let mut routes = Vec::new();
    'paths: for (path, item) in paths {
        for method in METHODS {
            let Some(operation) = item.get(method) else {
                continue;
            };
            if routes.len() >= MAX_ROUTES {
                notes.push(format!(
                    "Only the first {MAX_ROUTES} operations were imported."
                ));
                break 'paths;
            }
            routes.push(MockRoute {
                id: new_id(),
                method: method.to_string(),
                endpoint: endpoint_of(path),
                enabled: true,
                mode: ResponseMode::Rules,
                kind: RouteKind::Http,
                bucket: String::new(),
                id_key: default_id_key(),
                responses: responses_of(&root, operation, &mut budget),
            });
        }
    }
    if routes.is_empty() {
        return Err(AppError::invalid_input(
            "That OpenAPI document has no operations to mock.",
        ));
    }
    let title = root
        .pointer("/info/title")
        .and_then(Value::as_str)
        .unwrap_or("OpenAPI");
    let mock = MockEnvironment {
        name: title.chars().take(super::model::MAX_NAME_CHARS).collect(),
        port: 0,
        bind: super::model::MockBind::Loopback,
        prefix: prefix_of(&root),
        latency_ms: 0,
        cors: true,
        headers: Vec::new(),
        routes,
        tls: false,
        proxy: String::new(),
        data: Vec::new(),
        fallback: None,
    };
    Ok(Converted { mock, notes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::model::validate;

    const SPEC: &str = r##"
openapi: 3.0.3
info: { title: Pet store }
servers: [{ url: "https://api.example.com/v1" }]
paths:
  /pets:
    get:
      responses:
        "404": { description: missing }
        "200":
          description: all pets
          content:
            application/json:
              schema:
                type: array
                items: { $ref: "#/components/schemas/Pet" }
    post:
      responses:
        "201":
          description: made
          content:
            application/json:
              example: { id: 7 }
  /pets/{petId}/photos/{name}.png:
    delete:
      responses:
        default: { description: gone }
components:
  schemas:
    Pet:
      type: object
      properties:
        id: { type: integer }
        name: { type: string, example: Rex }
        born: { type: string, format: date }
        owner: { $ref: "#/components/schemas/Pet" }
"##;

    #[test]
    fn an_openapi_document_becomes_one_route_per_operation() {
        let done = import(SPEC).unwrap();
        let mock = &done.mock;
        validate("test", mock).unwrap();
        assert_eq!(mock.name, "Pet store");
        assert_eq!(mock.prefix, "v1");
        let shape: Vec<_> = mock
            .routes
            .iter()
            .map(|r| (r.method.as_str(), r.endpoint.as_str()))
            .collect();
        assert_eq!(
            shape,
            [
                ("get", "pets"),
                ("post", "pets"),
                ("delete", "pets/:petId/photos/*")
            ]
        );
        let list = &mock.routes[0].responses;
        assert_eq!(list[0].status, 200);
        assert!(list[0].default && !list[1].default);
        assert_eq!(list[1].status, 404);
        let body: Value = serde_json::from_str(&list[0].body).unwrap();
        assert_eq!(body[0]["name"], "Rex");
        assert_eq!(body[0]["born"], "2024-01-01");
        assert_eq!(body[0]["id"], 0);
        assert_eq!(list[0].headers[0].value, "application/json");
        assert_eq!(
            mock.routes[1].responses[0]
                .body
                .replace(char::is_whitespace, ""),
            "{\"id\":7}"
        );
        assert_eq!(mock.routes[2].responses[0].status, 200);
    }

    #[test]
    fn swagger_two_and_bad_input_are_handled() {
        let swagger = r#"{"swagger":"2.0","info":{"title":"S"},"basePath":"/api",
          "paths":{"/a":{"get":{"responses":{"200":{"description":"ok","schema":{"type":"string"}}}}}}}"#;
        let done = import(swagger).unwrap();
        assert_eq!(done.mock.prefix, "api");
        assert_eq!(done.mock.routes[0].responses[0].body, "string");
        assert!(import("{}").is_err());
        assert!(import("paths: {}").is_err());
        assert!(import(": : :").is_err());
    }

    #[test]
    fn a_self_referencing_schema_stops() {
        let spec = r##"{"openapi":"3.0.0","paths":{"/n":{"get":{"responses":{"200":{"description":"",
          "content":{"application/json":{"schema":{"$ref":"#/components/schemas/N"}}}}}}}},
          "components":{"schemas":{"N":{"type":"object","properties":{"next":{"$ref":"#/components/schemas/N"}}}}}}"##;
        let done = import(spec).unwrap();
        assert!(done.mock.routes[0].responses[0].body.len() < 5_000);
    }
}
