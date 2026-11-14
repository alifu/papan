use super::*;
use crate::engine::soap::model::PartRef;
use crate::engine::soap::test_support::fixture;
use crate::engine::soap::xml::{decode, parse, XmlLimits, MAX_DOCUMENT_BYTES};
use crate::engine::soap::SoapStyle;

fn read_into(
    definitions: &mut Definitions,
    name: &str,
    chameleon: Option<&str>,
) -> Result<Vec<ImportRef>, AppError> {
    let text = decode(&fixture(name), MAX_DOCUMENT_BYTES)?;
    let document = parse(&text, XmlLimits::default())?;
    definitions.read(&document, chameleon)?;
    Ok(imports(&document))
}

pub(crate) fn load(names: &[&str]) -> Result<(WsdlModel, Vec<String>), AppError> {
    let mut definitions = Definitions::default();
    for name in names {
        read_into(&mut definitions, name, None)?;
    }
    definitions.resolve()
}

fn port<'m>(model: &'m WsdlModel, name: &str) -> &'m Port {
    model
        .services
        .iter()
        .flat_map(|service| &service.ports)
        .find(|port| port.name == name)
        .unwrap()
}

#[test]
fn calculator_has_one_service_with_a_soap11_and_a_soap12_port() {
    let (model, warnings) = load(&["calculator.wsdl"]).unwrap();

    let soap11 = port(&model, "CalculatorSoap");
    let soap12 = port(&model, "CalculatorSoap12");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(model.services.len(), 1);
    assert_eq!(model.services[0].name, "Calculator");
    assert_eq!(soap11.version, SoapVersion::V11);
    assert_eq!(soap12.version, SoapVersion::V12);
    assert_eq!(soap11.address, "http://example.test/calculator.asmx");
    let add = &soap12.operations[0];
    assert_eq!(add.name, "Add");
    assert_eq!(add.action, "http://tempuri.org/Add");
    assert_eq!(add.style, SoapStyle::Document);
    assert_eq!(
        add.body,
        vec![Part {
            name: "parameters".into(),
            of: PartRef::Element {
                name: QName::new("http://tempuri.org/", "Add")
            }
        }]
    );
    assert_eq!(soap11.operations.len(), 2);
    assert_eq!(model.schema.elements.len(), 4);
}

#[test]
fn rpc_literal_takes_the_body_namespace_and_typed_parts() {
    let (model, warnings) = load(&["rpc-literal.wsdl"]).unwrap();

    let op = &port(&model, "GreeterPort").operations[0];
    assert!(warnings.is_empty());
    assert_eq!(op.style, SoapStyle::Rpc);
    assert!(!op.encoded);
    assert_eq!(op.namespace, "http://example.test/rpc");
    assert_eq!(op.body.len(), 2);
    assert!(matches!(&op.body[1].of, PartRef::Type { name } if name.name == "int"));
}

#[test]
fn rpc_encoded_is_read_with_a_warning() {
    let (model, warnings) = load(&["rpc-encoded.wsdl"]).unwrap();

    assert!(port(&model, "LegacyEchoPort").operations[0].encoded);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("rpc/encoded"), "{warnings:?}");
}

#[test]
fn soap_headers_are_resolved_to_their_message_part() {
    let (model, _) = load(&["headers.wsdl"]).unwrap();

    let op = &port(&model, "OrdersPort").operations[0];
    assert_eq!(op.headers.len(), 1);
    assert_eq!(op.headers[0].name, "Auth");
    assert!(matches!(&op.headers[0].of, PartRef::Element { name } if name.name == "AuthHeader"));
}

#[test]
fn an_http_binding_is_skipped_with_a_warning() {
    let (model, warnings) = load(&["http-binding.wsdl"]).unwrap();

    assert_eq!(model.services[0].ports.len(), 1);
    assert_eq!(model.services[0].ports[0].name, "CatalogSoapPort");
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("CatalogHttpGet"));
}

#[test]
fn wsdl_20_is_refused() {
    let error = load(&["bad-wsdl20.wsdl"]).unwrap_err();

    assert_eq!(error.message, "WSDL 2.0 is not supported.");
}

#[test]
fn a_missing_binding_is_an_error_naming_it() {
    let error = load(&["bad-no-binding.wsdl"]).unwrap_err();

    assert!(
        error.message.contains("MissingBinding"),
        "{}",
        error.message
    );
    assert!(error.message.contains("BrokenPort"));
}

#[test]
fn split_documents_list_their_imports_and_merge() {
    let mut definitions = Definitions::default();

    let main = read_into(&mut definitions, "split/main.wsdl", None).unwrap();
    let types = read_into(&mut definitions, "split/types.xsd", None).unwrap();
    read_into(
        &mut definitions,
        "split/common.xsd",
        types[0].chameleon.as_deref(),
    )
    .unwrap();
    let (model, warnings) = definitions.resolve().unwrap();

    assert_eq!(
        main,
        vec![ImportRef {
            location: "types.xsd".into(),
            chameleon: None
        }]
    );
    assert_eq!(
        types,
        vec![ImportRef {
            location: "common.xsd".into(),
            chameleon: Some("http://example.test/split/types".into())
        }]
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    let customer = QName::new("http://example.test/split/types", "Customer");
    assert!(model.schema.type_def(&customer).is_some());
    assert!(model
        .schema
        .element(&QName::new(
            "http://example.test/split/types",
            "GetCustomer"
        ))
        .is_some());
}

fn inline(text: &str) -> Result<(WsdlModel, Vec<String>), AppError> {
    let document = parse(text, XmlLimits::default())?;
    let mut definitions = Definitions::default();
    definitions.read(&document, None)?;
    definitions.resolve()
}

const HEAD: &str = r#"<definitions xmlns="http://schemas.xmlsoap.org/wsdl/" xmlns:soap="http://schemas.xmlsoap.org/wsdl/soap/" xmlns:t="urn:t" targetNamespace="urn:t">"#;

#[test]
fn wsdl_imports_are_listed_and_a_non_wsdl_root_is_refused() {
    let text = format!(r#"{HEAD}<import namespace="urn:x" location="other.wsdl"/></definitions>"#);
    let document = parse(&text, XmlLimits::default()).unwrap();

    assert_eq!(imports(&document)[0].location, "other.wsdl");
    assert!(inline(&text).unwrap_err().message.contains("no SOAP port"));
    assert!(inline("<a/>").unwrap_err().message.contains("not a WSDL"));
}

#[test]
fn missing_port_types_messages_and_parts_are_errors_naming_them() {
    let binding = r#"<binding name="B" type="t:PT"><soap:binding/><operation name="Op"><input><soap:body parts="a"/><soap:header message="t:M" part="nope"/></input></operation></binding>
        <service name="S"><port name="P" binding="t:B"><soap:address location="http://x"/></port></service>"#;
    let no_port_type = format!("{HEAD}{binding}</definitions>");
    let no_message = format!(
        r#"{HEAD}<portType name="PT"><operation name="Op"><input message="t:Missing"/></operation></portType>{binding}</definitions>"#
    );
    let no_part = format!(
        r#"{HEAD}<message name="M"><part name="a" type="t:X"/><part name="b"/></message><portType name="PT"><operation name="Op"><documentation> Says hi </documentation><input message="t:M"/></operation></portType>{binding}</definitions>"#
    );

    assert!(inline(&no_port_type)
        .unwrap_err()
        .message
        .contains("\"PT\""));
    assert!(inline(&no_message)
        .unwrap_err()
        .message
        .contains("\"Missing\""));
    assert!(inline(&no_part).unwrap_err().message.contains("\"nope\""));
}

#[test]
fn soap_body_parts_select_the_body_and_documentation_is_kept() {
    let text = format!(
        r#"{HEAD}<message name="M"><part name="a" type="t:X"/><part name="b"/></message><portType name="PT"><operation name="Op"><documentation> Says hi </documentation><input message="t:M"/></operation></portType>
        <binding name="B" type="t:PT"><soap:binding style="rpc"/><operation name="Op"><input><soap:body parts="a"/></input></operation></binding>
        <service name="S"><port name="P" binding="t:B"><soap:address location="http://x"/></port></service></definitions>"#
    );

    let (model, _) = inline(&text).unwrap();

    let op = &model.services[0].ports[0].operations[0];
    assert_eq!(op.body.len(), 1);
    assert_eq!(op.documentation, "Says hi");
    assert_eq!(op.style, SoapStyle::Rpc);
}
