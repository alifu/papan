use super::*;
use crate::engine::soap::model::{Operation, Port, Service, SimpleType};
use crate::engine::soap::test_support::model_of;
use crate::error::ErrorCode;

const ALL: EnvelopeOptions = EnvelopeOptions {
    include_optional: true,
};
const REQUIRED: EnvelopeOptions = EnvelopeOptions {
    include_optional: false,
};

/// Compares with `tests/fixtures/wsdl/expected/<golden>`; `UPDATE_GOLDEN=1` rewrites it.
fn golden(golden: &str, template: &SoapTemplate) {
    let path = format!(
        "{}/tests/fixtures/wsdl/expected/{golden}",
        env!("CARGO_MANIFEST_DIR")
    );
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &template.envelope).unwrap();
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(template.envelope, expected, "{golden}");
}

fn generate(
    files: &[&str],
    service: &str,
    port: &str,
    op: &str,
    options: EnvelopeOptions,
) -> SoapTemplate {
    let (model, _) = model_of(files);
    envelope(&model, service, port, op, options).unwrap()
}

#[test]
fn calculator_document_literal_soap_11() {
    let template = generate(
        &["calculator.wsdl"],
        "Calculator",
        "CalculatorSoap",
        "Add",
        ALL,
    );

    assert_eq!(template.version, SoapVersion::V11);
    assert_eq!(template.action, "http://tempuri.org/Add");
    assert_eq!(template.address, "http://example.test/calculator.asmx");
    golden("calculator-add-11.xml", &template);
}

#[test]
fn calculator_document_literal_soap_12() {
    let template = generate(
        &["calculator.wsdl"],
        "Calculator",
        "CalculatorSoap12",
        "Subtract",
        ALL,
    );

    assert_eq!(template.version, SoapVersion::V12);
    golden("calculator-subtract-12.xml", &template);
}

#[test]
fn rpc_literal_wraps_unqualified_parts_in_the_operation() {
    golden(
        "rpc-literal-sayhello.xml",
        &generate(
            &["rpc-literal.wsdl"],
            "Greeter",
            "GreeterPort",
            "SayHello",
            ALL,
        ),
    );
}

#[test]
fn rpc_encoded_is_generated_like_rpc_literal() {
    golden(
        "rpc-encoded-echo.xml",
        &generate(
            &["rpc-encoded.wsdl"],
            "LegacyEcho",
            "LegacyEchoPort",
            "echo",
            ALL,
        ),
    );
}

#[test]
fn soap_header_parts_go_into_the_header() {
    golden(
        "headers-placeorder.xml",
        &generate(&["headers.wsdl"], "Orders", "OrdersPort", "PlaceOrder", ALL),
    );
}

#[test]
fn types_zoo_with_optional_elements() {
    golden(
        "types-zoo-inventory.xml",
        &generate(&["types-zoo.wsdl"], "Zoo", "ZooPort", "Inventory", ALL),
    );
}

#[test]
fn types_zoo_without_optional_elements() {
    golden(
        "types-zoo-inventory-required.xml",
        &generate(&["types-zoo.wsdl"], "Zoo", "ZooPort", "Inventory", REQUIRED),
    );
}

#[test]
fn split_documents_resolve_across_imports_and_includes() {
    golden(
        "split-getcustomer.xml",
        &generate(
            &["split/main.wsdl", "split/types.xsd", "split/common.xsd"],
            "CustomerService",
            "CustomerPort",
            "GetCustomer",
            ALL,
        ),
    );
}

#[test]
fn unknown_service_port_or_operation_is_not_found_naming_it() {
    let (model, _) = model_of(&["calculator.wsdl"]);

    let errors = [
        envelope(&model, "Nope", "CalculatorSoap", "Add", ALL),
        envelope(&model, "Calculator", "Nope", "Add", ALL),
        envelope(&model, "Calculator", "CalculatorSoap", "Nope", ALL),
    ];

    for error in errors {
        let error = error.unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound);
        assert!(error.message.contains("\"Nope\""), "{}", error.message);
    }
}

#[test]
fn escape_covers_the_five_xml_characters() {
    assert_eq!(escape(r#"a&b<c>d"e'f"#), "a&amp;b&lt;c&gt;d&quot;e&apos;f");
    assert_eq!(comment("a--b"), "<!-- a- -b -->");
}

fn one_operation(schema: SchemaSet, body: Vec<Part>) -> WsdlModel {
    WsdlModel {
        services: vec![Service {
            name: "S".into(),
            ports: vec![Port {
                name: "P".into(),
                version: SoapVersion::V11,
                address: String::new(),
                operations: vec![Operation {
                    name: "Op".into(),
                    action: String::new(),
                    style: SoapStyle::Document,
                    encoded: false,
                    namespace: String::new(),
                    documentation: String::new(),
                    body,
                    headers: Vec::new(),
                }],
            }],
        }],
        schema,
    }
}

fn element_part(name: &str) -> Part {
    Part {
        name: "p".into(),
        of: PartRef::Element {
            name: QName::new("urn:t", name),
        },
    }
}

fn wide_type(name: &str, child_type: Option<&str>, width: usize) -> TypeDef {
    let items = (0..width)
        .map(|index| Particle::Element {
            decl: ElementDecl {
                name: format!("e{index}"),
                ns: String::new(),
                type_ref: child_type.map_or(TypeRef::Any, |child| TypeRef::Named {
                    name: QName::new("urn:t", child),
                }),
                occurs: Occurs::default(),
            },
        })
        .collect();
    TypeDef::Complex {
        def: ComplexType {
            label: name.into(),
            base: None,
            content: Some(Particle::Group {
                compositor: Compositor::Sequence,
                items,
                occurs: Occurs::default(),
            }),
            attributes: Vec::new(),
            simple: false,
        },
    }
}

fn root_element(type_name: &str) -> ElementDecl {
    ElementDecl {
        name: "Root".into(),
        ns: "urn:t".into(),
        type_ref: TypeRef::Named {
            name: QName::new("urn:t", type_name),
        },
        occurs: Occurs::default(),
    }
}

#[test]
fn an_envelope_over_one_mib_is_refused() {
    // Eight levels of 30 children each: far more than 1 MiB if it were all written.
    let mut schema = SchemaSet::default();
    for level in 0..8 {
        let child = (level < 7).then(|| format!("T{}", level + 1));
        schema.types.insert(
            QName::new("urn:t", format!("T{level}")).key(),
            wide_type(&format!("T{level}"), child.as_deref(), 30),
        );
    }
    schema
        .elements
        .insert(QName::new("urn:t", "Root").key(), root_element("T0"));

    let error = envelope(
        &one_operation(schema, vec![element_part("Root")]),
        "S",
        "P",
        "Op",
        ALL,
    )
    .unwrap_err();

    assert!(error.message.contains("1 MiB"));
}

#[test]
fn nesting_deeper_than_eight_types_stops_with_a_comment() {
    let mut schema = SchemaSet::default();
    for level in 0..12 {
        let child = format!("T{}", level + 1);
        schema.types.insert(
            QName::new("urn:t", format!("T{level}")).key(),
            wide_type(&format!("T{level}"), Some(&child), 1),
        );
    }
    schema
        .elements
        .insert(QName::new("urn:t", "Root").key(), root_element("T0"));

    let template = envelope(
        &one_operation(schema, vec![element_part("Root")]),
        "S",
        "P",
        "Op",
        ALL,
    )
    .unwrap();

    assert!(
        template.envelope.contains("<!-- recursive: T8 -->"),
        "{}",
        template.envelope
    );
    assert!(!template.envelope.contains("T9"));
}

#[test]
fn unknown_elements_and_empty_types_are_written_safely() {
    let mut schema = SchemaSet::default();
    schema.types.insert(
        QName::new("urn:t", "Empty").key(),
        TypeDef::Complex {
            def: ComplexType {
                label: "Empty".into(),
                base: None,
                content: None,
                attributes: vec![AttributeUse::Attribute {
                    name: "a".into(),
                    ns: "urn:other\"&".into(),
                    required: false,
                }],
                simple: false,
            },
        },
    );
    schema.types.insert(
        QName::new("urn:t", "E").key(),
        TypeDef::Simple {
            def: SimpleType {
                enumeration: vec!["x--y".into()],
                base: None,
            },
        },
    );
    schema
        .elements
        .insert(QName::new("urn:t", "Root").key(), root_element("Empty"));
    schema
        .elements
        .insert(QName::new("urn:t", "En").key(), root_element("E"));
    let model = one_operation(
        schema,
        vec![
            element_part("Missing"),
            element_part("Root"),
            element_part("En"),
        ],
    );

    let all = envelope(&model, "S", "P", "Op", ALL).unwrap().envelope;
    let required = envelope(&model, "S", "P", "Op", REQUIRED).unwrap().envelope;

    assert!(all.contains("<!-- unknown element: Missing -->"));
    assert!(all.contains("<ns1:Root ns2:a=\"?\"/>"), "{all}");
    assert!(all.contains("xmlns:ns2=\"urn:other&quot;&amp;\""));
    assert!(all.contains("<!-- one of: x- -y -->"));
    assert!(required.contains("<ns1:Root/>"), "{required}");
}

/// No `{{name}}` in `text` would resolve at send.
fn resolvable(text: &str) -> Vec<String> {
    crate::vars::template::names(text)
        .into_iter()
        .map(str::to_string)
        .collect()
}

#[test]
fn a_hostile_wsdl_cannot_inject_markup_or_variables() {
    let (model, warnings) = model_of(&["bad-injection.wsdl"]);

    let template = envelope(&model, "Injection", "InjectionPort", "Send", ALL).unwrap();

    assert!(!template.envelope.contains("<inj"), "{}", template.envelope);
    assert!(!template.envelope.contains(" b="), "{}", template.envelope);
    assert!(template.envelope.contains("<ns1:ok>?</ns1:ok>"));
    assert!(template.envelope.contains("\\{{secretToken}}"));
    for text in [&template.envelope, &template.action, &template.address] {
        assert!(resolvable(text).is_empty(), "{text}");
    }
    assert!(
        warnings
            .iter()
            .filter(|w| w.contains("not a valid XML name"))
            .count()
            >= 3,
        "{warnings:?}"
    );
}

fn group_of(items: Vec<Particle>) -> Particle {
    Particle::Group {
        compositor: Compositor::Sequence,
        items,
        occurs: Occurs::default(),
    }
}

fn group_ref(name: &str) -> Particle {
    Particle::GroupRef {
        name: QName::new("urn:t", name),
        occurs: Occurs::default(),
    }
}

fn root_with_content(content: Particle, attributes: Vec<AttributeUse>) -> SchemaSet {
    let mut schema = SchemaSet::default();
    let def = ComplexType {
        label: "T".into(),
        base: None,
        content: Some(content),
        attributes,
        simple: false,
    };
    schema
        .types
        .insert(QName::new("urn:t", "T").key(), TypeDef::Complex { def });
    schema
        .elements
        .insert(QName::new("urn:t", "Root").key(), root_element("T"));
    schema
}

fn timed(
    model: &WsdlModel,
    options: EnvelopeOptions,
) -> (Result<SoapTemplate, AppError>, std::time::Duration) {
    let started = std::time::Instant::now();
    let result = envelope(model, "S", "P", "Op", options);
    (result, started.elapsed())
}

#[test]
fn a_self_referencing_group_is_cut_at_once() {
    let leaf = Particle::Element {
        decl: ElementDecl {
            name: "a".into(),
            ns: String::new(),
            type_ref: TypeRef::Any,
            occurs: Occurs::default(),
        },
    };
    let mut schema = root_with_content(group_ref("G"), Vec::new());
    let items = std::iter::repeat_with(|| group_ref("G"))
        .take(12)
        .chain([leaf])
        .collect();
    schema
        .groups
        .insert(QName::new("urn:t", "G").key(), group_of(items));
    let model = one_operation(schema, vec![element_part("Root")]);

    let (result, took) = timed(&model, ALL);

    assert!(took < std::time::Duration::from_millis(500), "{took:?}");
    assert_eq!(result.unwrap().envelope.matches("<a>?</a>").count(), 1);
}

#[test]
fn a_self_referencing_attribute_group_is_cut_at_once() {
    let mut schema = root_with_content(
        group_of(Vec::new()),
        vec![AttributeUse::GroupRef {
            name: QName::new("urn:t", "AG"),
        }],
    );
    let uses = std::iter::repeat_with(|| AttributeUse::GroupRef {
        name: QName::new("urn:t", "AG"),
    })
    .take(12)
    .chain([AttributeUse::Attribute {
        name: "x".into(),
        ns: String::new(),
        required: true,
    }])
    .collect();
    schema
        .attribute_groups
        .insert(QName::new("urn:t", "AG").key(), uses);
    let model = one_operation(schema, vec![element_part("Root")]);

    let (result, took) = timed(&model, ALL);

    assert!(took < std::time::Duration::from_millis(500), "{took:?}");
    assert!(result.unwrap().envelope.contains("<ns1:Root x=\"?\"/>"));
}

#[test]
fn a_wide_schema_that_writes_nothing_still_hits_the_step_budget() {
    // G0 → 30 × G1 → … → G7 → 30 optional `any`: 30^8 steps, no output without optionals.
    let mut schema = root_with_content(group_ref("G0"), Vec::new());
    for level in 0..8 {
        let child = if level < 7 {
            group_ref(&format!("G{}", level + 1))
        } else {
            Particle::Any {
                occurs: Occurs {
                    min: 0,
                    max: Some(1),
                },
            }
        };
        let items = std::iter::repeat_n(child, 30).collect();
        schema.groups.insert(
            QName::new("urn:t", format!("G{level}")).key(),
            group_of(items),
        );
    }
    let model = one_operation(schema, vec![element_part("Root")]);

    let (result, took) = timed(&model, REQUIRED);

    assert!(took < std::time::Duration::from_secs(2), "{took:?}");
    assert!(result.unwrap_err().message.contains("steps"));
}
