use serde_json::json;

use super::*;
use crate::error::ErrorCode;
use crate::store::model::RequestFile;
use crate::store::protocol_model;
use crate::store::request_emit::{canonical_extra, merge};
use crate::store::request_model::{parse_sections, validate_content, Extra};
use crate::store::types::{GrpcSource, KeyValue};

fn extra(value: Value) -> Extra {
    value
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn header(name: &str, value: &str) -> KeyValue {
    KeyValue {
        name: name.into(),
        value: value.into(),
        enabled: true,
    }
}

fn grpc_content() -> RequestContent {
    RequestContent {
        method: "POST".into(),
        url: "https://{{host}}:443".into(),
        kind: RequestKind::Grpc,
        grpc: GrpcSettings {
            protos: vec!["greeter/v1/greeter.proto".into()],
            service: "greeter.v1.Greeter".into(),
            method: "SayHello".into(),
            ..GrpcSettings::default()
        },
        ..RequestContent::default()
    }
}

fn with_grpc(change: impl FnOnce(&mut GrpcSettings)) -> RequestContent {
    let mut content = grpc_content();
    change(&mut content.grpc);
    content
}

fn assert_names(error: &crate::error::AppError, field: &str) {
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(error.message.contains(field), "{}", error.message);
    assert!(error.message.contains("r.http.yaml"), "{}", error.message);
}

fn rejected(content: &RequestContent, field: &str) {
    let error = validate_content("r.http.yaml", content).unwrap_err();
    assert_names(&error, field);
}

#[test]
fn parses_the_full_section() {
    let raw = extra(json!({"grpc": {
        "deadline_ms": 30000, "import_paths": [".", "third_party"],
        "message": "{ \"name\": \"{{user}}\" }", "method": "SayHello",
        "protos": ["greeter/v1/greeter.proto"], "service": "greeter.v1.Greeter",
        "source": "reflection"
    }}));

    let content = parse_sections("r.http.yaml", RequestKind::Grpc, &raw).unwrap();

    assert_eq!(
        content.grpc,
        GrpcSettings {
            source: GrpcSource::Reflection,
            protos: vec!["greeter/v1/greeter.proto".into()],
            import_paths: vec![".".into(), "third_party".into()],
            service: "greeter.v1.Greeter".into(),
            method: "SayHello".into(),
            message: "{ \"name\": \"{{user}}\" }".into(),
            deadline_ms: 30000,
        }
    );
}

#[test]
fn missing_section_is_default() {
    let content = parse_sections("r.http.yaml", RequestKind::Grpc, &Extra::new()).unwrap();

    assert_eq!(content.grpc, GrpcSettings::default());
}

#[test]
fn emit_is_canonical_omits_defaults_and_is_a_fixed_point() {
    let raw = extra(json!({"grpc": {
        "service": "a.B", "method": "C", "protos": ["a.proto"], "source": "protos",
        "import_paths": [], "deadline_ms": 0, "message": ""
    }}));

    let once = canonical_extra("r.http.yaml", RequestKind::Grpc, &raw).unwrap();
    let twice = canonical_extra("r.http.yaml", RequestKind::Grpc, &once).unwrap();

    assert_eq!(
        once["grpc"],
        json!({"method": "C", "protos": ["a.proto"], "service": "a.B"})
    );
    assert_eq!(once, twice);
}

#[test]
fn an_all_default_section_is_omitted() {
    let raw = extra(json!({"grpc": {"source": "protos"}}));

    let once = canonical_extra("r.http.yaml", RequestKind::Grpc, &raw).unwrap();

    assert!(!once.contains_key("grpc"));
}

#[test]
fn unknown_keys_survive_canonicalising_and_unchanged_merges() {
    let raw = extra(json!({"grpc": {"service": "a.B", "future": {"x": 1}}}));
    let disk = parse_sections("r.http.yaml", RequestKind::Grpc, &raw).unwrap();

    let canonical = canonical_extra("r.http.yaml", RequestKind::Grpc, &raw).unwrap();
    let unchanged = merge("r.http.yaml", RequestKind::Grpc, &raw, &disk).unwrap();
    let edited = RequestContent {
        grpc: GrpcSettings {
            method: "C".into(),
            ..disk.grpc.clone()
        },
        ..disk
    };
    let changed = merge("r.http.yaml", RequestKind::Grpc, &raw, &edited).unwrap();

    assert_eq!(canonical["grpc"]["future"], json!({"x": 1}));
    assert_eq!(unchanged["grpc"], raw["grpc"]);
    assert_eq!(changed["grpc"]["method"], "C");
}

#[test]
fn a_whole_request_file_round_trips_through_yaml() {
    let content = RequestContent {
        headers: vec![
            header("x-tenant", "{{tenant}}"),
            header("trace-bin", "AAEC"),
        ],
        ..with_grpc(|g| {
            g.deadline_ms = 5000;
            g.import_paths = vec![".".into()];
            g.message = "{}".into();
        })
    };
    let file = RequestFile::new("Say hello".into(), "POST", RequestKind::Grpc)
        .with_content(&content)
        .unwrap();

    let yaml = file.to_yaml().unwrap();
    let back = RequestFile::parse("r.http.yaml", yaml.as_bytes()).unwrap();

    assert_eq!(back.content().unwrap(), content);
    assert_eq!(back.to_yaml().unwrap(), yaml);
    assert!(yaml.contains("kind: grpc"));
}

#[test]
fn bad_shapes_name_the_field() {
    for (value, field) in [
        (json!("x"), "grpc"),
        (json!({"source": "files"}), "grpc.source"),
        (json!({"protos": "a.proto"}), "grpc.protos"),
        (json!({"protos": [1]}), "grpc.protos"),
        (json!({"import_paths": {"a": 1}}), "grpc.import_paths"),
        (json!({"deadline_ms": -1}), "grpc.deadline_ms"),
        (json!({"deadline_ms": "soon"}), "grpc.deadline_ms"),
        (json!({"service": ["a"]}), "grpc.service"),
    ] {
        let error = parse_sections(
            "r.http.yaml",
            RequestKind::Grpc,
            &extra(json!({"grpc": value})),
        )
        .unwrap_err();

        assert_names(&error, field);
    }
}

#[test]
fn the_section_is_refused_in_other_kinds() {
    let raw = extra(json!({"grpc": {"service": "a.B"}}));
    for kind in [
        RequestKind::Http,
        RequestKind::Graphql,
        RequestKind::Soap,
        RequestKind::Ws,
    ] {
        let error = parse_sections("r.http.yaml", kind, &raw).unwrap_err();

        assert_names(&error, "grpc");
    }
    let typed = RequestContent {
        grpc: grpc_content().grpc,
        ..RequestContent::new("GET", "https://x")
    };
    rejected(&typed, "grpc");
}

#[test]
fn other_kind_sections_are_refused_in_grpc() {
    for (section, field) in [
        (json!({"scripts": {"pre_request": "x"}}), "scripts"),
        (json!({"ws": {"subprotocols": ["a"]}}), "ws"),
        (json!({"subscription": {"url": "wss://x"}}), "subscription"),
        (json!({"soap": {"action": "a"}}), "soap"),
    ] {
        let error = parse_sections("r.http.yaml", RequestKind::Grpc, &extra(section)).unwrap_err();

        assert_names(&error, field);
    }
    let body = extra(json!({"body": {"type": "json", "text": "{}"}}));
    let error = parse_sections("r.http.yaml", RequestKind::Grpc, &body).unwrap_err();
    assert_names(&error, "body.type");
}

#[test]
fn the_method_must_be_post() {
    let error = protocol_model::check_method("r.http.yaml", RequestKind::Grpc, "GET").unwrap_err();

    assert_names(&error, "method");
    assert!(protocol_model::check_method("r.http.yaml", RequestKind::Grpc, "POST").is_ok());
}

#[test]
fn urls_allow_a_scheme_and_host_only() {
    for ok in [
        "",
        "http://localhost:50051",
        "HTTPS://host:443/",
        "{{base}}",
        "https://{{host}}:443",
    ] {
        assert!(check_url("r.http.yaml", ok).is_ok(), "{ok}");
    }
    for bad in [
        "ftp://x",
        "host:50051",
        "ws://x",
        "https://x/path",
        "https://x/?a=1",
        "https://x?a=1",
        "https://x#f",
    ] {
        assert_names(&check_url("r.http.yaml", bad).unwrap_err(), "url");
    }
}

#[test]
fn service_and_method_names_are_checked() {
    for service in ["a.B", "Greeter", "_a.b_2.C"] {
        let content = with_grpc(|g| g.service = service.into());
        assert!(
            validate_content("r.http.yaml", &content).is_ok(),
            "{service}"
        );
    }
    for service in ["a..B", ".a", "a.", "a b", "1a", "a.b-c", &"a".repeat(257)] {
        rejected(&with_grpc(|g| g.service = service.into()), "grpc.service");
    }
    for method in ["Say Hello", "1x", "a.b", &"m".repeat(129)] {
        rejected(&with_grpc(|g| g.method = method.into()), "grpc.method");
    }
    let long_ok = with_grpc(|g| g.method = "m".repeat(128));
    assert!(validate_content("r.http.yaml", &long_ok).is_ok());
}

#[test]
fn proto_paths_are_relative_proto_files() {
    for good in ["a.proto", "greeter/v1/greeter.proto"] {
        let content = with_grpc(|g| g.protos = vec![good.into()]);
        assert!(validate_content("r.http.yaml", &content).is_ok(), "{good}");
    }
    for bad in [
        "../a.proto",
        "/etc/a.proto",
        "a/../b.proto",
        "a\\b.proto",
        "c:/a.proto",
        "a.txt",
        "a/",
        "",
        ".hidden.proto",
        "a/./b.proto",
        &format!("{}.proto", "a".repeat(513)),
    ] {
        rejected(&with_grpc(|g| g.protos = vec![bad.into()]), "grpc.protos");
    }
}

#[test]
fn import_paths_are_relative_directories() {
    for good in [".", "third_party", "a/b"] {
        let content = with_grpc(|g| g.import_paths = vec![good.into()]);
        assert!(validate_content("r.http.yaml", &content).is_ok(), "{good}");
    }
    for bad in ["..", "../x", "/abs", "a/../b", "a\\b", "", &"a".repeat(513)] {
        rejected(
            &with_grpc(|g| g.import_paths = vec![bad.into()]),
            "grpc.import_paths",
        );
    }
}

#[test]
fn counts_and_sizes_are_limited() {
    let many_protos: Vec<String> = (0..51).map(|n| format!("p{n}.proto")).collect();
    rejected(&with_grpc(|g| g.protos = many_protos), "grpc.protos");
    let many_imports: Vec<String> = (0..21).map(|n| format!("d{n}")).collect();
    rejected(
        &with_grpc(|g| g.import_paths = many_imports),
        "grpc.import_paths",
    );
    rejected(
        &with_grpc(|g| g.message = "x".repeat(MAX_MESSAGE_BYTES + 1)),
        "grpc.message",
    );
    rejected(
        &with_grpc(|g| g.deadline_ms = MAX_DEADLINE_MS + 1),
        "grpc.deadline_ms",
    );
    let edge = with_grpc(|g| {
        g.protos = (0..50).map(|n| format!("p{n}.proto")).collect();
        g.import_paths = (0..20).map(|n| format!("d{n}")).collect();
        g.message = "x".repeat(MAX_MESSAGE_BYTES);
        g.deadline_ms = MAX_DEADLINE_MS;
    });
    assert!(validate_content("r.http.yaml", &edge).is_ok());
}

#[test]
fn a_message_may_hold_newlines() {
    let content = with_grpc(|g| g.message = "{\n  \"a\": 1\n}".into());

    assert!(validate_content("r.http.yaml", &content).is_ok());
}

#[test]
fn metadata_names_must_be_lowercase_and_not_reserved() {
    for bad in [
        "X-Tenant",
        ":authority",
        "content-type",
        "te",
        "host",
        "connection",
        "grpc-timeout",
        "grpc-encoding",
        "grpc-anything",
    ] {
        let content = RequestContent {
            headers: vec![header(bad, "v")],
            ..grpc_content()
        };
        rejected(&content, "headers");
    }
    let content = RequestContent {
        headers: vec![
            header("x-tenant", "a"),
            header("authorization", "Bearer x"),
            header("{{name}}", "v"),
        ],
        ..grpc_content()
    };
    assert!(validate_content("r.http.yaml", &content).is_ok());
}

#[test]
fn reserved_names_are_fine_in_other_kinds() {
    let content = RequestContent {
        headers: vec![header("Content-Type", "x"), header("Host", "y")],
        ..RequestContent::new("POST", "https://x")
    };

    assert!(validate_content("r.http.yaml", &content).is_ok());
}

#[test]
fn bin_metadata_values_must_be_base64_unless_variables() {
    for ok in ["AAEC", "AAE", "AAE=", "", "{{blob}}", "pre{{x}}"] {
        let content = RequestContent {
            headers: vec![header("trace-bin", ok)],
            ..grpc_content()
        };
        assert!(validate_content("r.http.yaml", &content).is_ok(), "{ok:?}");
    }
    for bad in ["not base64!", "AA=E", "A"] {
        let content = RequestContent {
            headers: vec![header("trace-bin", bad)],
            ..grpc_content()
        };
        rejected(&content, "headers.value");
    }
    assert!(is_valid_bin_value("AAEC"));
    assert!(!is_valid_bin_value("**"));
}

#[test]
fn metadata_is_limited_to_100_entries() {
    let rows = |n: usize| (0..n).map(|i| header(&format!("k{i}"), "v")).collect();
    let ok = RequestContent {
        headers: rows(100),
        ..grpc_content()
    };
    let over = RequestContent {
        headers: rows(101),
        ..grpc_content()
    };

    assert!(validate_content("r.http.yaml", &ok).is_ok());
    rejected(&over, "headers");
}
