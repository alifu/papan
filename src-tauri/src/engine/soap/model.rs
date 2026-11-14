//! The parsed WSDL model (plan 06 §7): what the envelope generator needs, resolved across
//! every imported document. It is what the cache stores (never the raw documents), so it
//! is plain serde data. Maps are keyed by a QName in Clark notation (`{ns}local`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::SoapStyle;
use crate::store::types::SoapVersion;

pub const XSD_NS: &str = "http://www.w3.org/2001/XMLSchema";

/// A namespace-qualified name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct QName {
    pub ns: String,
    pub name: String,
}

impl QName {
    pub fn new(ns: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            ns: ns.into(),
            name: name.into(),
        }
    }

    /// `{ns}local`, the key of the schema maps.
    pub fn key(&self) -> String {
        format!("{{{}}}{}", self.ns, self.name)
    }

    pub fn is_xsd(&self) -> bool {
        self.ns == XSD_NS
    }
}

/// `maxOccurs`; `None` is `unbounded`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Occurs {
    pub min: u32,
    pub max: Option<u32>,
}

impl Default for Occurs {
    fn default() -> Self {
        Self {
            min: 1,
            max: Some(1),
        }
    }
}

impl Occurs {
    pub fn is_optional(&self) -> bool {
        self.min == 0
    }

    pub fn is_repeated(&self) -> bool {
        self.max.is_none_or(|max| max > 1)
    }
}

/// The type of an element or attribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeRef {
    /// No type given: `xs:anyType`.
    Any,
    Named {
        name: QName,
    },
    Inline {
        def: Box<TypeDef>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElementDecl {
    pub name: String,
    /// Empty when the element is unqualified.
    pub ns: String,
    pub type_ref: TypeRef,
    pub occurs: Occurs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Compositor {
    Sequence,
    All,
    Choice,
}

/// A piece of a complex type's content model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Particle {
    Element {
        decl: ElementDecl,
    },
    /// `<element ref="…">`: the global element, with these occurrences.
    ElementRef {
        name: QName,
        occurs: Occurs,
    },
    Group {
        compositor: Compositor,
        items: Vec<Particle>,
        occurs: Occurs,
    },
    /// `<group ref="…">`.
    GroupRef {
        name: QName,
        occurs: Occurs,
    },
    Any {
        occurs: Occurs,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttributeUse {
    Attribute {
        name: String,
        /// Empty when unqualified.
        ns: String,
        required: bool,
    },
    GroupRef {
        name: QName,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Derivation {
    Extension,
    Restriction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComplexType {
    /// The name used in recursion comments (`anonymous` for inline types).
    pub label: String,
    pub base: Option<(QName, Derivation)>,
    pub content: Option<Particle>,
    pub attributes: Vec<AttributeUse>,
    /// `simpleContent`: text plus attributes.
    pub simple: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimpleType {
    pub enumeration: Vec<String>,
    pub base: Option<QName>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeDef {
    Simple { def: SimpleType },
    Complex { def: ComplexType },
}

/// Every global schema component of an import, keyed by `QName::key`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaSet {
    pub elements: BTreeMap<String, ElementDecl>,
    pub types: BTreeMap<String, TypeDef>,
    pub groups: BTreeMap<String, Particle>,
    pub attribute_groups: BTreeMap<String, Vec<AttributeUse>>,
}

impl SchemaSet {
    pub fn element(&self, name: &QName) -> Option<&ElementDecl> {
        self.elements.get(&name.key())
    }

    pub fn type_def(&self, name: &QName) -> Option<&TypeDef> {
        self.types.get(&name.key())
    }
}

/// One message part, by global element or by type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PartRef {
    Element { name: QName },
    Type { name: QName },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Part {
    pub name: String,
    pub of: PartRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    pub name: String,
    pub action: String,
    pub style: SoapStyle,
    pub encoded: bool,
    /// `soap:body@namespace` (the rpc wrapper's namespace).
    pub namespace: String,
    pub documentation: String,
    pub body: Vec<Part>,
    pub headers: Vec<Part>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Port {
    pub name: String,
    pub version: SoapVersion,
    pub address: String,
    pub operations: Vec<Operation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Service {
    pub name: String,
    pub ports: Vec<Port>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WsdlModel {
    pub services: Vec<Service>,
    pub schema: SchemaSet,
}
