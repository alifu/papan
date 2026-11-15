//! What a picked file is (plan 09 §3 "Detection", first match wins).

use serde_json::Value;

use crate::error::AppError;
use crate::interop::postman::limits::file_error;
use crate::interop::postman::types::PostmanFileKind;

const V21: &str = "v2.1.0";
const V20: &str = "v2.0.0";

pub fn detect(file: &str, value: &Value) -> Result<PostmanFileKind, AppError> {
    let Value::Object(map) = value else {
        return Err(not_postman(file));
    };
    if let Some(info) = map.get("info").and_then(Value::as_object) {
        let schema = info
            .get("schema")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if schema.contains(V21) {
            return Ok(PostmanFileKind::CollectionV21);
        }
        if schema.contains(V20) {
            return Ok(PostmanFileKind::CollectionV20);
        }
        if schema.contains("v1") || !map.contains_key("item") {
            return Err(v1(file));
        }
        return Err(not_postman(file));
    }
    if map.contains_key("requests") && !map.contains_key("item") {
        return Err(v1(file));
    }
    if map.get("values").is_some_and(Value::is_array) {
        return variable_file_kind(file, map);
    }
    if map.contains_key("collections") || map.contains_key("environments") {
        return Err(file_error(
            file,
            "Data dumps are not supported; export collections one by one.",
        ));
    }
    Err(not_postman(file))
}

fn variable_file_kind(
    file: &str,
    map: &serde_json::Map<String, Value>,
) -> Result<PostmanFileKind, AppError> {
    match map.get("_postman_variable_scope").and_then(Value::as_str) {
        Some("globals") => Ok(PostmanFileKind::Globals),
        Some("environment") => Ok(PostmanFileKind::Environment),
        None if map.contains_key("name") => Ok(PostmanFileKind::Environment),
        _ => Err(not_postman(file)),
    }
}

fn v1(file: &str) -> AppError {
    file_error(
        file,
        "Postman v1 collections are not supported; re-export as v2.1.",
    )
}

fn not_postman(file: &str) -> AppError {
    file_error(file, "Not a Postman collection or environment.")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn detection_table() {
        let ok = [
            (
                json!({"info": {"schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"}, "item": []}),
                PostmanFileKind::CollectionV21,
            ),
            (
                json!({"info": {"schema": "https://schema.getpostman.com/json/collection/v2.0.0/collection.json"}, "item": []}),
                PostmanFileKind::CollectionV20,
            ),
            (
                json!({"name": "G", "values": [], "_postman_variable_scope": "globals"}),
                PostmanFileKind::Globals,
            ),
            (
                json!({"name": "Dev", "values": [], "_postman_variable_scope": "environment"}),
                PostmanFileKind::Environment,
            ),
            (
                json!({"name": "Dev", "values": []}),
                PostmanFileKind::Environment,
            ),
        ];
        for (value, kind) in ok {
            assert_eq!(detect("f.json", &value).unwrap(), kind, "{value}");
        }
    }

    #[test]
    fn refusals_name_the_file_and_the_reason() {
        let cases = [
            (json!({"info": {"schema": "v1"}}), "v1"),
            (json!({"id": "x", "requests": []}), "v1"),
            (json!({"collections": [], "environments": []}), "Data dumps"),
            (json!([1, 2]), "Not a Postman"),
            (json!("text"), "Not a Postman"),
            (
                json!({"values": [], "_postman_variable_scope": "other"}),
                "Not a Postman",
            ),
            (
                json!({"info": {"schema": "x"}, "item": []}),
                "Not a Postman",
            ),
            (json!({"a": 1}), "Not a Postman"),
        ];
        for (value, reason) in cases {
            let error = detect("f.json", &value).unwrap_err();
            assert!(error.message.starts_with("f.json: "), "{}", error.message);
            assert!(error.message.contains(reason), "{}", error.message);
        }
    }
}
