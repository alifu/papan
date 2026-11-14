//! WSDL 1.1 (plan 06 §7.1). Every fetched document adds its messages, port types,
//! bindings, services and schemas to one [`Definitions`]; [`resolve`] then follows
//! port → binding → portType → message → part into the [`WsdlModel`].

use std::collections::BTreeMap;

use roxmltree::{Document, Node};

use super::model::{Operation, Part, Port, QName, SchemaSet, Service, WsdlModel, XSD_NS};
use super::xml::{child_named, children_named, elements, required_name, resolve_qname};
use super::xsd::read_schema;

#[path = "wsdl_read.rs"]
mod read;
use crate::error::AppError;
use crate::store::types::SoapVersion;
use read::{
    binding_op, message_parts, port_type_ops, service_ports, soap_child, style_of, Binding,
    PortTypeOp, RawPort,
};

pub const WSDL_NS: &str = "http://schemas.xmlsoap.org/wsdl/";
pub const WSDL20_NS: &str = "http://www.w3.org/ns/wsdl";
pub const SOAP11_NS: &str = "http://schemas.xmlsoap.org/wsdl/soap/";
pub const SOAP12_NS: &str = "http://schemas.xmlsoap.org/wsdl/soap12/";

/// An import found in a document: where, and what namespace an `include` inherits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRef {
    pub location: String,
    pub chameleon: Option<String>,
}

/// Everything read so far, across documents.
#[derive(Debug, Clone, Default)]
pub struct Definitions {
    messages: BTreeMap<String, Vec<Part>>,
    port_types: BTreeMap<String, Vec<PortTypeOp>>,
    bindings: BTreeMap<String, Binding>,
    services: Vec<(String, Vec<RawPort>)>,
    pub schema: SchemaSet,
    pub warnings: Vec<String>,
}

/// The imports of a document, in document order.
pub fn imports(document: &Document<'_>) -> Vec<ImportRef> {
    let root = document.root_element();
    let wsdl = children_named(root, WSDL_NS, "import").filter_map(|node| {
        Some(ImportRef {
            location: node.attribute("location")?.to_string(),
            chameleon: None,
        })
    });
    let schemas = root
        .descendants()
        .filter(|node| node.has_tag_name((XSD_NS, "schema")))
        .flat_map(|schema| {
            let tns = schema.attribute("targetNamespace").map(str::to_string);
            elements(schema).filter_map(move |node| {
                let chameleon = match node.tag_name().name() {
                    "include" => tns.clone(),
                    "import" => None,
                    _ => return None,
                };
                (node.tag_name().namespace() == Some(XSD_NS)).then_some(())?;
                Some(ImportRef {
                    location: node.attribute("schemaLocation")?.to_string(),
                    chameleon,
                })
            })
        });
    wsdl.chain(schemas).collect()
}

impl Definitions {
    fn warn(&mut self, text: String) {
        if !self.warnings.contains(&text) {
            self.warnings.push(text);
        }
    }

    /// Adds one fetched document: a WSDL 1.1 `definitions` or a bare `xs:schema`.
    pub fn read(
        &mut self,
        document: &Document<'_>,
        chameleon: Option<&str>,
    ) -> Result<(), AppError> {
        let root = document.root_element();
        match (root.tag_name().namespace(), root.tag_name().name()) {
            (Some(XSD_NS), "schema") => {
                read_schema(root, chameleon, &mut self.schema, &mut self.warnings);
                Ok(())
            }
            (Some(WSDL_NS), "definitions") => self.read_definitions(root),
            (Some(WSDL20_NS), _) => Err(AppError::invalid_input("WSDL 2.0 is not supported.")),
            _ => Err(AppError::invalid_input(
                "The document is not a WSDL 1.1 or XML Schema document.",
            )),
        }
    }

    fn read_definitions(&mut self, root: Node<'_, '_>) -> Result<(), AppError> {
        let tns = root.attribute("targetNamespace").unwrap_or_default();
        if let Some(types) = child_named(root, WSDL_NS, "types") {
            for schema in children_named(types, XSD_NS, "schema") {
                read_schema(schema, None, &mut self.schema, &mut self.warnings);
            }
        }
        for node in elements(root).filter(|node| node.tag_name().namespace() == Some(WSDL_NS)) {
            // A broken component (a bad name, an undeclared prefix) is skipped, not fatal.
            if let Err(error) = self.read_component(node, tns) {
                self.warn(error.message);
            }
        }
        Ok(())
    }

    fn read_component(&mut self, node: Node<'_, '_>, tns: &str) -> Result<(), AppError> {
        let key = |name: &str| QName::new(tns, name).key();
        match node.tag_name().name() {
            "message" => {
                let parts = message_parts(node)?;
                self.messages
                    .insert(key(required_name(node, "message")?), parts);
            }
            "portType" => {
                let operations = port_type_ops(node)?;
                self.port_types
                    .insert(key(required_name(node, "portType")?), operations);
            }
            "binding" => {
                let name = required_name(node, "binding")?;
                let binding = self.binding(node, name)?;
                self.bindings.insert(key(name), binding);
            }
            "service" => {
                let ports = service_ports(node)?;
                self.services
                    .push((required_name(node, "service")?.to_string(), ports));
            }
            _ => {}
        }
        Ok(())
    }

    fn binding(&mut self, node: Node<'_, '_>, name: &str) -> Result<Binding, AppError> {
        let port_type = resolve_qname(node, node.attribute("type").unwrap_or_default())?;
        let soap = soap_child(node, "binding");
        let version = soap.map(|soap| match soap.tag_name().namespace() {
            Some(SOAP12_NS) => SoapVersion::V12,
            _ => SoapVersion::V11,
        });
        if version.is_none() {
            self.warn(format!(
                "Binding \"{name}\" is not a SOAP binding and was skipped."
            ));
        }
        let style = style_of(soap.and_then(|soap| soap.attribute("style"))).unwrap_or_default();
        let operations = children_named(node, WSDL_NS, "operation")
            .map(binding_op)
            .collect::<Result<_, _>>()?;
        Ok(Binding {
            port_type,
            version,
            style,
            operations,
        })
    }

    /// Follows every SOAP port to its operations. A port naming a missing binding,
    /// port type or message is an error that names it.
    pub fn resolve(self) -> Result<(WsdlModel, Vec<String>), AppError> {
        let mut warnings = self.warnings.clone();
        let mut services = Vec::new();
        for (name, raw_ports) in &self.services {
            let mut ports = Vec::new();
            for raw in raw_ports {
                let binding = self.bindings.get(&raw.binding.key()).ok_or_else(|| {
                    AppError::invalid_input(format!(
                        "Port \"{}\" uses binding \"{}\", which is not defined.",
                        raw.name, raw.binding.name
                    ))
                })?;
                let Some(version) = binding.version else {
                    continue;
                };
                let operations = self.operations(binding, &raw.name, &mut warnings)?;
                ports.push(Port {
                    name: raw.name.clone(),
                    version,
                    address: raw.address.clone(),
                    operations,
                });
            }
            if !ports.is_empty() {
                services.push(Service {
                    name: name.clone(),
                    ports,
                });
            }
        }
        if services.is_empty() {
            return Err(AppError::invalid_input("The WSDL defines no SOAP port."));
        }
        let model = WsdlModel {
            services,
            schema: self.schema,
        };
        Ok((model, warnings))
    }

    fn operations(
        &self,
        binding: &Binding,
        port: &str,
        warnings: &mut Vec<String>,
    ) -> Result<Vec<Operation>, AppError> {
        let port_type = self
            .port_types
            .get(&binding.port_type.key())
            .ok_or_else(|| {
                AppError::invalid_input(format!(
                    "Port type \"{}\" is not defined.",
                    binding.port_type.name
                ))
            })?;
        binding
            .operations
            .iter()
            .map(|op| {
                let abstract_op = port_type.iter().find(|candidate| candidate.name == op.name);
                let parts = match abstract_op.and_then(|found| found.input.as_ref()) {
                    Some(message) => self.message(message)?.clone(),
                    None => Vec::new(),
                };
                let body = match &op.body_parts {
                    Some(names) => parts.into_iter().filter(|part| names.contains(&part.name)).collect(),
                    None => parts,
                };
                let headers = op
                    .headers
                    .iter()
                    .map(|(message, part)| {
                        self.message(message)?
                            .iter()
                            .find(|candidate| &candidate.name == part)
                            .cloned()
                            .ok_or_else(|| {
                                AppError::invalid_input(format!(
                                    "Message \"{}\" has no part \"{part}\".",
                                    message.name
                                ))
                            })
                    })
                    .collect::<Result<_, _>>()?;
                let style = op.style.unwrap_or(binding.style);
                if op.encoded {
                    let text = format!(
                        "Operation \"{}\" of port \"{port}\" is rpc/encoded: its envelope is generated as literal (no xsi:type, no SOAP-ENC arrays).",
                        op.name
                    );
                    if !warnings.contains(&text) {
                        warnings.push(text);
                    }
                }
                Ok(Operation {
                    name: op.name.clone(),
                    action: op.action.clone(),
                    style,
                    encoded: op.encoded,
                    namespace: op.namespace.clone(),
                    documentation: abstract_op.map(|found| found.documentation.clone()).unwrap_or_default(),
                    body,
                    headers,
                })
            })
            .collect()
    }

    fn message(&self, name: &QName) -> Result<&Vec<Part>, AppError> {
        self.messages.get(&name.key()).ok_or_else(|| {
            AppError::invalid_input(format!("Message \"{}\" is not defined.", name.name))
        })
    }
}

#[cfg(test)]
#[path = "wsdl_tests.rs"]
mod tests;
