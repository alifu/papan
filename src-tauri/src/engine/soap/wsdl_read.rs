//! The raw WSDL 1.1 elements read from one document, before cross-references are
//! followed (split from `wsdl.rs`).

use roxmltree::Node;

use super::{SOAP11_NS, SOAP12_NS, WSDL_NS};
use crate::engine::soap::model::{Part, PartRef, QName, XSD_NS};
use crate::engine::soap::xml::{
    child_named, children_named, elements, required_name, resolve_qname,
};
use crate::engine::soap::SoapStyle;
use crate::error::AppError;
use crate::store::types::SoapVersion;

#[derive(Debug, Clone, Default)]
pub(super) struct PortTypeOp {
    pub(super) name: String,
    pub(super) input: Option<QName>,
    pub(super) documentation: String,
}

#[derive(Debug, Clone, Default)]
pub(super) struct BindingOp {
    pub(super) name: String,
    pub(super) action: String,
    pub(super) style: Option<SoapStyle>,
    pub(super) encoded: bool,
    pub(super) namespace: String,
    pub(super) body_parts: Option<Vec<String>>,
    pub(super) headers: Vec<(QName, String)>,
}

#[derive(Debug, Clone)]
pub(super) struct Binding {
    pub(super) port_type: QName,
    /// `None` for an HTTP or MIME binding.
    pub(super) version: Option<SoapVersion>,
    pub(super) style: SoapStyle,
    pub(super) operations: Vec<BindingOp>,
}

#[derive(Debug, Clone)]
pub(super) struct RawPort {
    pub(super) name: String,
    pub(super) binding: QName,
    pub(super) address: String,
}

pub(super) fn text_of(node: Option<Node<'_, '_>>) -> String {
    node.map(|node| {
        node.descendants()
            .filter(Node::is_text)
            .filter_map(|text| text.text())
            .collect::<String>()
            .trim()
            .to_string()
    })
    .unwrap_or_default()
}

pub(super) fn soap_child<'a, 'input>(
    node: Node<'a, 'input>,
    name: &'a str,
) -> Option<Node<'a, 'input>> {
    child_named(node, SOAP11_NS, name).or_else(|| child_named(node, SOAP12_NS, name))
}

pub(super) fn style_of(value: Option<&str>) -> Option<SoapStyle> {
    match value {
        Some("rpc") => Some(SoapStyle::Rpc),
        Some("document") => Some(SoapStyle::Document),
        _ => None,
    }
}

pub(super) fn message_parts(node: Node<'_, '_>) -> Result<Vec<Part>, AppError> {
    children_named(node, WSDL_NS, "part")
        .map(|part| {
            let name = required_name(part, "message part")?.to_string();
            let of = match (part.attribute("element"), part.attribute("type")) {
                (Some(element), _) => PartRef::Element {
                    name: resolve_qname(part, element)?,
                },
                (None, Some(type_name)) => PartRef::Type {
                    name: resolve_qname(part, type_name)?,
                },
                (None, None) => PartRef::Type {
                    name: QName::new(XSD_NS, "anyType"),
                },
            };
            Ok(Part { name, of })
        })
        .collect()
}

pub(super) fn port_type_ops(node: Node<'_, '_>) -> Result<Vec<PortTypeOp>, AppError> {
    children_named(node, WSDL_NS, "operation")
        .map(|op| {
            let input = child_named(op, WSDL_NS, "input")
                .and_then(|input| {
                    input
                        .attribute("message")
                        .map(|value| resolve_qname(input, value))
                })
                .transpose()?;
            Ok(PortTypeOp {
                name: required_name(op, "operation")?.to_string(),
                input,
                documentation: text_of(child_named(op, WSDL_NS, "documentation")),
            })
        })
        .collect()
}

pub(super) fn binding_op(op: Node<'_, '_>) -> Result<BindingOp, AppError> {
    let soap_op = soap_child(op, "operation");
    let input = child_named(op, WSDL_NS, "input");
    let body = input.and_then(|input| soap_child(input, "body"));
    let headers = input
        .into_iter()
        .flat_map(|input| {
            elements(input).filter(|node| {
                node.tag_name().name() == "header"
                    && matches!(node.tag_name().namespace(), Some(SOAP11_NS | SOAP12_NS))
            })
        })
        .map(|header| {
            let message = resolve_qname(header, header.attribute("message").unwrap_or_default())?;
            Ok((
                message,
                header.attribute("part").unwrap_or_default().to_string(),
            ))
        })
        .collect::<Result<_, AppError>>()?;
    Ok(BindingOp {
        name: required_name(op, "binding operation")?.to_string(),
        action: soap_op
            .and_then(|node| node.attribute("soapAction"))
            .unwrap_or_default()
            .to_string(),
        style: style_of(soap_op.and_then(|node| node.attribute("style"))),
        encoded: body.and_then(|node| node.attribute("use")) == Some("encoded"),
        namespace: body
            .and_then(|node| node.attribute("namespace"))
            .unwrap_or_default()
            .to_string(),
        body_parts: body
            .and_then(|node| node.attribute("parts"))
            .map(|parts| parts.split_whitespace().map(str::to_string).collect()),
        headers,
    })
}

pub(super) fn service_ports(node: Node<'_, '_>) -> Result<Vec<RawPort>, AppError> {
    children_named(node, WSDL_NS, "port")
        .map(|port| {
            Ok(RawPort {
                name: required_name(port, "port")?.to_string(),
                binding: resolve_qname(port, port.attribute("binding").unwrap_or_default())?,
                address: elements(port)
                    .find(|child| child.tag_name().name() == "address")
                    .and_then(|address| address.attribute("location"))
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect()
}
