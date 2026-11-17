//! Envelope generation (plan 06 §7.2): `?` placeholders with SoapUI-style comments,
//! namespace prefixes `ns1`, `ns2`… in first-use order declared on the `Envelope`, and
//! deterministic output so golden tests compare bytes.

use super::model::{
    AttributeUse, ComplexType, Compositor, Derivation, ElementDecl, Occurs, Part, PartRef,
    Particle, QName, SchemaSet, TypeDef, TypeRef, WsdlModel,
};
use super::xml::neutralise;

#[path = "envelope_parts.rs"]
mod parts;
use super::{EnvelopeOptions, SoapStyle, SoapTemplate};
use crate::error::AppError;
use crate::store::types::SoapVersion;
pub use parts::escape;
use parts::{comment, find, label};

pub const MAX_ENVELOPE_BYTES: usize = 1024 * 1024;
pub const MAX_DEPTH: usize = 8;
/// Elements and particles visited for one envelope: bounds the work of wide schemas
/// that write little (optional content left out).
pub const MAX_STEPS: usize = 100_000;
/// Bounds following `base` and attribute-group chains (which may be cyclic).
const MAX_CHAIN: usize = 32;
const INDENT: &str = "  ";

struct Writer<'m> {
    schema: &'m SchemaSet,
    include_optional: bool,
    namespaces: Vec<String>,
    lines: Vec<String>,
    bytes: usize,
    /// Named types and groups on the current path, for recursion.
    path: Vec<Option<String>>,
    steps: usize,
}

/// Attributes and content of a complex type with its extension bases folded in.
struct Flat {
    attributes: Vec<(String, String, bool)>,
    content: Vec<Particle>,
    simple: bool,
}

impl<'m> Writer<'m> {
    fn over(&self) -> bool {
        self.bytes > MAX_ENVELOPE_BYTES || self.steps > MAX_STEPS
    }

    /// Counts one visit; true when the budget is spent or the output is full.
    fn spent(&mut self) -> bool {
        self.steps += 1;
        self.over()
    }

    fn line(&mut self, depth: usize, text: String) {
        if self.over() {
            return;
        }
        self.bytes += depth * INDENT.len() + text.len() + 1;
        self.lines.push(format!("{}{text}", INDENT.repeat(depth)));
    }

    fn prefixed(&mut self, ns: &str, name: &str) -> String {
        if ns.is_empty() {
            return name.to_string();
        }
        let index = match self.namespaces.iter().position(|known| known == ns) {
            Some(index) => index,
            None => {
                self.namespaces.push(ns.to_string());
                self.namespaces.len() - 1
            }
        };
        format!("ns{}:{name}", index + 1)
    }

    fn occurrence(&mut self, depth: usize, occurs: Occurs) {
        let text = match (occurs.min, occurs.is_repeated()) {
            (0, true) => "Zero or more repetitions:".to_string(),
            (min, true) => format!("{min} or more repetitions:"),
            (0, false) => "Optional:".to_string(),
            _ => return,
        };
        self.line(depth, comment(&text));
    }

    fn skipped(&self, occurs: Occurs) -> bool {
        occurs.is_optional() && !self.include_optional
    }

    fn element(&mut self, decl: &ElementDecl, occurs: Occurs, depth: usize) {
        if self.spent() || self.skipped(occurs) {
            return;
        }
        self.occurrence(depth, occurs);
        let tag = self.prefixed(&decl.ns, &decl.name);
        let (def, key) = match &decl.type_ref {
            TypeRef::Any => (None, None),
            TypeRef::Inline { def } => (Some(def.as_ref()), None),
            TypeRef::Named { name } if name.is_xsd() => (None, None),
            TypeRef::Named { name } => (self.schema.type_def(name), Some(name.key())),
        };
        match def {
            None => self.line(depth, format!("<{tag}>?</{tag}>")),
            Some(TypeDef::Simple { def }) => {
                if !def.enumeration.is_empty() {
                    let text = format!("one of: {}", def.enumeration.join(", "));
                    self.line(depth, comment(&text));
                }
                self.line(depth, format!("<{tag}>?</{tag}>"));
            }
            Some(TypeDef::Complex { def }) => {
                let recursive = key.is_some() && self.path.contains(&key);
                if recursive || self.path.len() >= MAX_DEPTH {
                    let text = comment(&format!("recursive: {}", def.label));
                    return self.line(depth, format!("<{tag}>{text}</{tag}>"));
                }
                self.path.push(key);
                self.complex(&tag, def, depth);
                self.path.pop();
            }
        }
    }

    fn complex(&mut self, tag: &str, def: &ComplexType, depth: usize) {
        let flat = self.flatten(def);
        let wanted: Vec<(String, String)> = flat
            .attributes
            .iter()
            .filter(|(_, _, required)| *required || self.include_optional)
            .map(|(ns, name, _)| (ns.clone(), name.clone()))
            .collect();
        let attributes: String = wanted
            .iter()
            .map(|(ns, name)| format!(" {}=\"?\"", self.prefixed(ns, name)))
            .collect();
        if flat.simple {
            return self.line(depth, format!("<{tag}{attributes}>?</{tag}>"));
        }
        if flat.content.is_empty() {
            return self.line(depth, format!("<{tag}{attributes}/>"));
        }
        self.line(depth, format!("<{tag}{attributes}>"));
        let before = self.lines.len();
        for particle in &flat.content {
            self.particle(particle, depth + 1);
        }
        if self.lines.len() == before {
            self.lines.pop();
            self.line(depth, format!("<{tag}{attributes}/>"));
        } else {
            self.line(depth, format!("</{tag}>"));
        }
    }

    /// Folds extension bases (outermost first) and attribute groups into one list.
    fn flatten(&self, def: &ComplexType) -> Flat {
        let mut chain = vec![def];
        while let Some((base, Derivation::Extension)) = &chain[chain.len() - 1].base {
            match self.schema.type_def(base) {
                Some(TypeDef::Complex { def }) if chain.len() < MAX_CHAIN => chain.push(def),
                _ => break,
            }
        }
        let simple = chain.iter().any(|def| def.simple);
        let mut attributes = Vec::new();
        let mut expanded = Vec::new();
        for def in chain.iter().rev() {
            self.attributes(&def.attributes, &mut attributes, &mut expanded);
        }
        Flat {
            attributes,
            content: chain
                .iter()
                .rev()
                .filter_map(|def| def.content.clone())
                .collect(),
            simple: simple && chain.iter().all(|def| def.content.is_none()),
        }
    }

    /// Each attribute group is expanded once per type (`expanded`), which also cuts cycles.
    fn attributes(
        &self,
        uses: &[AttributeUse],
        out: &mut Vec<(String, String, bool)>,
        expanded: &mut Vec<String>,
    ) {
        for attribute in uses {
            match attribute {
                AttributeUse::Attribute { name, ns, required } => {
                    out.push((ns.clone(), name.clone(), *required));
                }
                AttributeUse::GroupRef { name } => {
                    let key = name.key();
                    if expanded.contains(&key) {
                        continue;
                    }
                    expanded.push(key);
                    if let Some(group) = self.schema.attribute_groups.get(&name.key()) {
                        self.attributes(group, out, expanded);
                    }
                }
            }
        }
    }

    fn particle(&mut self, particle: &Particle, depth: usize) {
        if self.spent() {
            return;
        }
        match particle {
            Particle::Element { decl } => self.element(decl, decl.occurs, depth),
            Particle::ElementRef { name, occurs } => self.global(name, *occurs, depth),
            Particle::Any { occurs } => {
                if !self.skipped(*occurs) {
                    self.line(depth, comment("any element"));
                }
            }
            Particle::GroupRef { name, occurs } => {
                let group = self.schema.groups.get(&name.key());
                let key = Some(format!("group:{}", name.key()));
                // A group already on the path refers to itself: it is cut at once.
                let cyclic = self.path.contains(&key);
                match group {
                    Some(group)
                        if !cyclic && !self.skipped(*occurs) && self.path.len() < MAX_DEPTH =>
                    {
                        self.path.push(key);
                        self.particle(group, depth);
                        self.path.pop();
                    }
                    _ => {}
                }
            }
            Particle::Group {
                compositor,
                items,
                occurs,
            } => {
                if self.skipped(*occurs) {
                    return;
                }
                if *compositor == Compositor::Choice {
                    let labels: Vec<String> = items.iter().map(label).collect();
                    self.line(depth, comment(&format!("choice: {}", labels.join(" | "))));
                    if let Some(first) = items.first() {
                        self.particle(first, depth);
                    }
                    return;
                }
                for item in items {
                    self.particle(item, depth);
                }
            }
        }
    }

    fn global(&mut self, name: &QName, occurs: Occurs, depth: usize) {
        match self.schema.element(name) {
            Some(decl) => self.element(decl, occurs, depth),
            None => self.line(depth, comment(&format!("unknown element: {}", name.name))),
        }
    }

    /// A part as a document child (global element, or an unqualified element of a type).
    fn part(&mut self, part: &Part, depth: usize) {
        match &part.of {
            PartRef::Element { name } => self.global(name, Occurs::default(), depth),
            PartRef::Type { name } => {
                let decl = ElementDecl {
                    name: part.name.clone(),
                    ns: String::new(),
                    type_ref: TypeRef::Named { name: name.clone() },
                    occurs: Occurs::default(),
                };
                self.element(&decl, decl.occurs, depth);
            }
        }
    }

    fn take(&mut self) -> Vec<String> {
        std::mem::take(&mut self.lines)
    }
}

/// The request envelope of one operation.
pub fn envelope(
    model: &WsdlModel,
    service: &str,
    port: &str,
    operation: &str,
    options: EnvelopeOptions,
) -> Result<SoapTemplate, AppError> {
    let (port, op) = find(model, service, port, operation)?;
    let (env_prefix, env_ns) = match port.version {
        SoapVersion::V11 => ("soapenv", "http://schemas.xmlsoap.org/soap/envelope/"),
        SoapVersion::V12 => ("soap", "http://www.w3.org/2003/05/soap-envelope"),
    };
    let mut writer = Writer {
        schema: &model.schema,
        include_optional: options.include_optional,
        namespaces: Vec::new(),
        lines: Vec::new(),
        bytes: 0,
        path: Vec::new(),
        steps: 0,
    };
    for part in &op.headers {
        writer.part(part, 2);
    }
    let header = writer.take();
    match op.style {
        SoapStyle::Document => op.body.iter().for_each(|part| writer.part(part, 2)),
        SoapStyle::Rpc => {
            let wrapper = writer.prefixed(&op.namespace, &op.name);
            writer.line(2, format!("<{wrapper}>"));
            op.body.iter().for_each(|part| writer.part(part, 3));
            writer.line(2, format!("</{wrapper}>"));
        }
    }
    let body = writer.take();
    if writer.steps > MAX_STEPS {
        return Err(AppError::invalid_input(format!(
            "The envelope needs more than {MAX_STEPS} steps: the schema is too large or too nested."
        )));
    }
    if writer.over() {
        return Err(AppError::invalid_input(
            "The envelope is larger than 1 MiB.",
        ));
    }
    let declarations: String = writer
        .namespaces
        .iter()
        .enumerate()
        .map(|(index, ns)| format!(" xmlns:ns{}=\"{}\"", index + 1, escape(ns)))
        .collect();
    let open_header = if header.is_empty() {
        vec![format!("{INDENT}<{env_prefix}:Header/>")]
    } else {
        std::iter::once(format!("{INDENT}<{env_prefix}:Header>"))
            .chain(header)
            .chain(std::iter::once(format!("{INDENT}</{env_prefix}:Header>")))
            .collect()
    };
    let lines: Vec<String> = std::iter::once(format!(
        "<{env_prefix}:Envelope xmlns:{env_prefix}=\"{env_ns}\"{declarations}>"
    ))
    .chain(open_header)
    .chain(std::iter::once(format!("{INDENT}<{env_prefix}:Body>")))
    .chain(body)
    .chain([
        format!("{INDENT}</{env_prefix}:Body>"),
        format!("</{env_prefix}:Envelope>"),
    ])
    .collect();
    Ok(SoapTemplate {
        version: port.version,
        // Text from the WSDL must never resolve as a `{{variable}}` when the request is sent.
        action: neutralise(&op.action),
        address: neutralise(&port.address),
        envelope: neutralise(&(lines.join("\n") + "\n")),
    })
}

#[cfg(test)]
#[path = "envelope_tests.rs"]
mod tests;
