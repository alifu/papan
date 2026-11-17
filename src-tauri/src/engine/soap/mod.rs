//! SOAP helpers that are not sends: WSDL import, the model cache and envelope
//! generation (plan 06 §7). Sending a SOAP request goes through the HTTP pipeline.

pub mod cache;
pub mod commands;
pub mod envelope;
pub mod fetch;
pub mod model;
pub mod rules;
pub mod service;
pub mod walk;
pub mod wsdl;
pub mod xml;
pub mod xsd;

#[cfg(test)]
pub(crate) mod test_support {
    /// A WSDL fixture of `tests/fixtures/wsdl`.
    pub fn fixture(name: &str) -> Vec<u8> {
        let path = format!("{}/tests/fixtures/wsdl/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
    }

    /// The model of fixture documents read in order; an `include`d document takes the
    /// namespace its includer gave it.
    pub fn model_of(names: &[&str]) -> (super::model::WsdlModel, Vec<String>) {
        use super::wsdl::{imports, Definitions};
        use super::xml::{decode, parse, XmlLimits, MAX_DOCUMENT_BYTES};
        let mut definitions = Definitions::default();
        let mut chameleons = std::collections::HashMap::new();
        for name in names {
            let text = decode(&fixture(name), MAX_DOCUMENT_BYTES).unwrap();
            let document = parse(&text, XmlLimits::default()).unwrap();
            let base = name.rsplit('/').next().unwrap_or(name);
            let chameleon: Option<String> = chameleons.get(base).cloned().flatten();
            definitions.read(&document, chameleon.as_deref()).unwrap();
            for import in imports(&document) {
                chameleons.insert(import.location.clone(), import.chameleon.clone());
            }
        }
        definitions.resolve().unwrap()
    }
}

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::store::types::SoapVersion;

/// Where a WSDL import starts: a URL typed by the user or a file picked in Rust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsdlSource {
    Url { url: String },
    PickFile,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SoapStyle {
    #[default]
    Document,
    Rpc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct WsdlOperationInfo {
    pub name: String,
    pub action: String,
    pub style: SoapStyle,
    pub documentation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct WsdlPortInfo {
    pub name: String,
    pub version: SoapVersion,
    pub address: String,
    pub operations: Vec<WsdlOperationInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct WsdlServiceInfo {
    pub name: String,
    pub ports: Vec<WsdlPortInfo>,
}

/// A parsed WSDL. `source` and `fetched` are masked locations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct WsdlImport {
    pub id: String,
    pub source: String,
    /// What a request saves as `soap.wsdl.url` to find this import again; empty for a
    /// picked file (then the file name in `source` is saved as `soap.wsdl.file`).
    pub ref_url: String,
    pub services: Vec<WsdlServiceInfo>,
    pub fetched: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EnvelopeOptions {
    pub include_optional: bool,
}

impl Default for EnvelopeOptions {
    fn default() -> Self {
        Self {
            include_optional: true,
        }
    }
}

/// A generated request: `envelope` goes to the body, `action`/`version` to `soap`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SoapTemplate {
    pub version: SoapVersion,
    pub action: String,
    pub address: String,
    pub envelope: String,
}
