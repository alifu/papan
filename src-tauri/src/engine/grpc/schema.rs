//! A descriptor pool as the picker sees it (plan 08 §5): services and methods, sorted,
//! with a proto3 JSON template of every request message.

use prost_reflect::{
    DescriptorPool, FieldDescriptor, Kind, MessageDescriptor, MethodDescriptor, ServiceDescriptor,
};
use serde_json::{json, Map, Value};

use super::types::{GrpcMethod, GrpcMethodKind, GrpcSchema, GrpcService};
use crate::error::AppError;
use crate::store::types::GrpcSource;

/// Nested messages are filled in down to this depth; deeper ones are `{}`.
pub const MAX_TEMPLATE_DEPTH: usize = 4;
/// A template stops growing after this many values (a hostile proto can fan out to
/// billions); the fields past it are left out.
pub const MAX_TEMPLATE_NODES: usize = 2000;
/// A template longer than this becomes `{}`.
pub const MAX_TEMPLATE_BYTES: usize = 64 * 1024;
/// All the templates of one schema together stay under this many bytes; later methods
/// get `{}`.
pub const MAX_SCHEMA_TEMPLATE_BYTES: usize = 4 * 1024 * 1024;
pub const TEMPLATE_CUT_WARNING: &str =
    "Some request templates are incomplete because the messages are very large.";
const REFLECTION_PREFIX: &str = "grpc.reflection.";

pub fn method_kind(method: &MethodDescriptor) -> GrpcMethodKind {
    match (method.is_client_streaming(), method.is_server_streaming()) {
        (false, false) => GrpcMethodKind::Unary,
        (false, true) => GrpcMethodKind::ServerStreaming,
        (true, false) => GrpcMethodKind::ClientStreaming,
        (true, true) => GrpcMethodKind::Bidi,
    }
}

fn method_of(method: &MethodDescriptor, budget: &mut usize, cut: &mut bool) -> GrpcMethod {
    let (template, truncated) = if *budget == 0 {
        ("{}".to_string(), true)
    } else {
        template_checked(&method.input())
    };
    *budget = budget.saturating_sub(template.len());
    *cut |= truncated;
    GrpcMethod {
        name: method.name().to_string(),
        kind: method_kind(method),
        input_type: method.input().full_name().to_string(),
        output_type: method.output().full_name().to_string(),
        template,
    }
}

fn service_of(service: &ServiceDescriptor, budget: &mut usize, cut: &mut bool) -> GrpcService {
    let mut methods: Vec<GrpcMethod> = service
        .methods()
        .map(|m| method_of(&m, budget, cut))
        .collect();
    methods.sort_by(|a, b| a.name.cmp(&b.name));
    GrpcService {
        full_name: service.full_name().to_string(),
        methods,
    }
}

/// The services of `pool` (the reflection service itself is hidden), sorted by name.
pub fn schema_of(
    pool: &DescriptorPool,
    source: GrpcSource,
    files: Vec<String>,
    fetched_at_ms: Option<f64>,
) -> GrpcSchema {
    let (mut budget, mut cut) = (MAX_SCHEMA_TEMPLATE_BYTES, false);
    let mut services: Vec<GrpcService> = pool
        .services()
        .filter(|service| !service.full_name().starts_with(REFLECTION_PREFIX))
        .map(|service| service_of(&service, &mut budget, &mut cut))
        .collect();
    services.sort_by(|a, b| a.full_name.cmp(&b.full_name));
    GrpcSchema {
        source,
        services,
        files,
        warnings: if cut {
            vec![TEMPLATE_CUT_WARNING.to_string()]
        } else {
            Vec::new()
        },
        fetched_at_ms,
    }
}

/// The method `service`/`method` names, or an error that says which part is unknown.
pub fn find_method(
    pool: &DescriptorPool,
    service: &str,
    method: &str,
) -> Result<MethodDescriptor, AppError> {
    if service.is_empty() || method.is_empty() {
        return Err(AppError::invalid_input("Choose a service and a method."));
    }
    let found = pool.get_service_by_name(service).ok_or_else(|| {
        AppError::invalid_input(format!("The service `{service}` is not defined."))
    })?;
    let result = found.methods().find(|candidate| candidate.name() == method);
    result.ok_or_else(|| {
        AppError::invalid_input(format!("The service `{service}` has no method `{method}`."))
    })
}

/// `/pkg.Service/Method`.
pub fn path_of(method: &MethodDescriptor) -> String {
    format!("/{}/{}", method.parent_service().full_name(), method.name())
}

/// Pretty proto3 JSON for `message` with every field present (first member of a oneof).
pub fn template(message: &MessageDescriptor) -> String {
    template_checked(message).0
}

/// The template and whether it was cut short by the node or byte cap.
pub fn template_checked(message: &MessageDescriptor) -> (String, bool) {
    let mut nodes = MAX_TEMPLATE_NODES;
    let value = message_value(message, 0, &mut Vec::new(), &mut nodes);
    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string());
    if text.len() > MAX_TEMPLATE_BYTES {
        return ("{}".to_string(), true);
    }
    (text, nodes == 0)
}

fn well_known(name: &str) -> Option<Value> {
    Some(match name {
        "google.protobuf.Timestamp" => json!("1970-01-01T00:00:00Z"),
        "google.protobuf.Duration" => json!("0s"),
        "google.protobuf.DoubleValue"
        | "google.protobuf.FloatValue"
        | "google.protobuf.Int32Value"
        | "google.protobuf.UInt32Value" => json!(0),
        "google.protobuf.Int64Value" | "google.protobuf.UInt64Value" => json!("0"),
        "google.protobuf.BoolValue" => json!(false),
        "google.protobuf.StringValue"
        | "google.protobuf.BytesValue"
        | "google.protobuf.FieldMask" => json!(""),
        "google.protobuf.Struct" | "google.protobuf.Any" | "google.protobuf.Empty" => json!({}),
        "google.protobuf.Value" => Value::Null,
        "google.protobuf.ListValue" => json!([]),
        _ => return None,
    })
}

fn message_value(
    message: &MessageDescriptor,
    depth: usize,
    open: &mut Vec<String>,
    nodes: &mut usize,
) -> Value {
    let name = message.full_name();
    if let Some(value) = well_known(name) {
        return value;
    }
    if depth > MAX_TEMPLATE_DEPTH || open.iter().any(|seen| seen == name) {
        return json!({});
    }
    open.push(name.to_string());
    let mut fields = Map::new();
    for field in message.fields().filter(is_shown) {
        if *nodes == 0 {
            break;
        }
        *nodes -= 1;
        let value = field_value(&field, depth, open, nodes);
        fields.insert(field.json_name().to_string(), value);
    }
    open.pop();
    Value::Object(fields)
}

/// Every field, except the later members of a real oneof (only one may be set).
fn is_shown(field: &FieldDescriptor) -> bool {
    match field.containing_oneof() {
        Some(oneof) if !oneof.is_synthetic() => oneof
            .fields()
            .next()
            .is_some_and(|first| first.number() == field.number()),
        _ => true,
    }
}

fn field_value(
    field: &FieldDescriptor,
    depth: usize,
    open: &mut Vec<String>,
    nodes: &mut usize,
) -> Value {
    if field.is_map() {
        return json!({});
    }
    let element = match field.kind() {
        Kind::Double | Kind::Float => json!(0),
        Kind::Int32 | Kind::Uint32 | Kind::Sint32 | Kind::Fixed32 | Kind::Sfixed32 => json!(0),
        Kind::Int64 | Kind::Uint64 | Kind::Sint64 | Kind::Fixed64 | Kind::Sfixed64 => json!("0"),
        Kind::Bool => json!(false),
        Kind::String | Kind::Bytes => json!(""),
        Kind::Enum(kind) if kind.full_name() == "google.protobuf.NullValue" => Value::Null,
        Kind::Enum(kind) => {
            let first = kind.values().next();
            first.map_or(json!(0), |value| json!(value.name()))
        }
        Kind::Message(nested) => message_value(&nested, depth + 1, open, nodes),
    };
    if field.is_list() {
        json!([element])
    } else {
        element
    }
}

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
