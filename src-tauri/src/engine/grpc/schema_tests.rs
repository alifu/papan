use super::*;
use crate::engine::grpc::codec::from_json;
use crate::engine::grpc::protos::tests::pool_of;

const PROTO: &str = r#"syntax = "proto3";
package shop.v1;
import "google/protobuf/timestamp.proto";
import "google/protobuf/duration.proto";
import "google/protobuf/wrappers.proto";
import "google/protobuf/struct.proto";
import "google/protobuf/any.proto";

service Zeta { rpc Ping (Node) returns (Node); }
service Alpha {
  rpc Watch (Order) returns (stream Order);
  rpc Upload (stream Order) returns (Order);
  rpc Chat (stream Order) returns (stream Order);
  rpc Get (Order) returns (Order);
}
enum Size { SIZE_UNSPECIFIED = 0; LARGE = 1; }
message Node { string id = 1; Node next = 2; repeated Node children = 3; }
message L1 { L2 l2 = 1; } message L2 { L3 l3 = 1; } message L3 { L4 l4 = 1; }
message L4 { L5 l5 = 1; } message L5 { L6 l6 = 1; } message L6 { string end = 1; }
message Order {
  string order_id = 1;
  int32 quantity = 2;
  int64 cents = 3;
  double weight = 4;
  bool gift = 5;
  bytes blob = 6;
  Size size = 7;
  repeated string tags = 8;
  map<string, int32> counts = 9;
  oneof payment { string card = 10; string voucher = 11; }
  optional string note = 12;
  google.protobuf.Timestamp at = 13;
  google.protobuf.Duration ttl = 14;
  google.protobuf.Int64Value big = 15;
  google.protobuf.StringValue label = 16;
  google.protobuf.Struct extra = 17;
  google.protobuf.Value any_value = 18;
  google.protobuf.Any packed = 19;
  L1 deep = 20;
  google.protobuf.NullValue nothing = 21;
}
"#;

fn pool() -> DescriptorPool {
    pool_of(&[("shop.proto", PROTO)], "shop.proto")
}

fn template_of(name: &str) -> Value {
    let message = pool().get_message_by_name(name).unwrap();
    serde_json::from_str(&template(&message)).unwrap()
}

#[test]
fn services_and_methods_are_sorted_with_their_kinds() {
    let schema = schema_of(&pool(), GrpcSource::Protos, vec!["shop.proto".into()], None);

    let names: Vec<&str> = schema
        .services
        .iter()
        .map(|service| service.full_name.as_str())
        .collect();
    assert_eq!(names, ["shop.v1.Alpha", "shop.v1.Zeta"]);
    let kinds: Vec<(&str, GrpcMethodKind)> = schema.services[0]
        .methods
        .iter()
        .map(|method| (method.name.as_str(), method.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("Chat", GrpcMethodKind::Bidi),
            ("Get", GrpcMethodKind::Unary),
            ("Upload", GrpcMethodKind::ClientStreaming),
            ("Watch", GrpcMethodKind::ServerStreaming),
        ]
    );
    let get = &schema.services[0].methods[1];
    assert_eq!(get.input_type, "shop.v1.Order");
    assert_eq!(get.output_type, "shop.v1.Order");
    assert_eq!(schema.files, ["shop.proto"]);
}

#[test]
fn the_template_has_every_field_with_its_zero_value() {
    let order = template_of("shop.v1.Order");

    let expected = json!({
        "orderId": "", "quantity": 0, "cents": "0", "weight": 0, "gift": false, "blob": "",
        "size": "SIZE_UNSPECIFIED", "tags": [""], "counts": {}, "card": "", "note": "",
        "at": "1970-01-01T00:00:00Z", "ttl": "0s", "big": "0", "label": "", "extra": {},
        "anyValue": null, "packed": {}, "nothing": null,
        "deep": {"l2": {"l3": {"l4": {"l5": {}}}}},
    });
    assert_eq!(order, expected);
}

#[test]
fn a_recursive_message_stops_at_the_cycle() {
    let node = template_of("shop.v1.Node");

    assert_eq!(node, json!({"id": "", "next": {}, "children": [{}]}));
}

#[test]
fn the_template_is_accepted_as_a_message() {
    let message = pool().get_message_by_name("shop.v1.Order").unwrap();
    let mut value = template_of("shop.v1.Order");
    // `Any` needs a `@type` the template cannot know.
    value.as_object_mut().unwrap().remove("packed");

    let parsed = from_json(&message, &value.to_string());

    assert!(parsed.is_ok(), "{parsed:?}");
}

#[test]
fn methods_are_found_by_name_with_clear_errors() {
    let pool = pool();

    let found = find_method(&pool, "shop.v1.Alpha", "Watch").unwrap();
    assert_eq!(path_of(&found), "/shop.v1.Alpha/Watch");
    let message =
        |service: &str, method: &str| find_method(&pool, service, method).unwrap_err().message;
    assert!(message("", "").contains("Choose a service"));
    assert!(message("shop.v1.Nope", "Get").contains("`shop.v1.Nope` is not defined"));
    assert!(message("shop.v1.Alpha", "Nope").contains("no method `Nope`"));
}

#[test]
fn the_reflection_service_is_hidden() {
    let proto = "syntax = \"proto3\";\npackage grpc.reflection.v1;\nmessage M {}\n\
                 service ServerReflection { rpc Info (M) returns (M); }\n";
    let pool = pool_of(&[("r.proto", proto)], "r.proto");

    let schema = schema_of(&pool, GrpcSource::Reflection, Vec::new(), Some(1.0));

    assert!(schema.services.is_empty());
    assert_eq!(schema.fetched_at_ms, Some(1.0));
}

/// Five levels of 200 distinct message fields each: 200^5 values if left unchecked.
fn hostile_proto() -> String {
    const WIDTH: usize = 200;
    let mut text = String::from("syntax = \"proto3\";\npackage evil.v1;\n");
    text.push_str("service Evil { rpc Go (L0) returns (L0); }\n");
    for level in 0..5 {
        text.push_str(&format!("message L{level} {{\n"));
        for field in 0..WIDTH {
            text.push_str(&format!("  L{} f{field} = {};\n", level + 1, field + 1));
        }
        text.push_str("}\n");
    }
    text.push_str("message L5 { string end = 1; }\n");
    text
}

#[test]
fn a_hostile_proto_gets_a_bounded_template_and_a_warning() {
    let pool = pool_of(&[("evil.proto", &hostile_proto())], "evil.proto");

    let started = std::time::Instant::now();
    let schema = schema_of(&pool, GrpcSource::Protos, vec![], None);

    let text = &schema.services[0].methods[0].template;
    assert!(text.len() <= MAX_TEMPLATE_BYTES, "{}", text.len());
    assert!(serde_json::from_str::<Value>(text).is_ok());
    assert!(schema.warnings.iter().any(|w| w == TEMPLATE_CUT_WARNING));
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}
