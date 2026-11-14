//! The XSD subset the envelope generator needs (plan 06 §7.1), read into a [`SchemaSet`].
//! Unknown constructs become warnings, never failures: the envelope stays editable.

use roxmltree::Node;

use super::model::{
    AttributeUse, ComplexType, Compositor, Derivation, ElementDecl, Occurs, Particle, QName,
    SchemaSet, SimpleType, TypeDef, TypeRef, XSD_NS,
};
use super::xml::{elements, is_ncname, neutralise, resolve_qname};

/// What one `xs:schema` element's local declarations inherit.
struct Context<'w> {
    tns: String,
    elements_qualified: bool,
    attributes_qualified: bool,
    warnings: &'w mut Vec<String>,
}

fn is_xsd(node: Node<'_, '_>, name: &str) -> bool {
    node.has_tag_name((XSD_NS, name))
}

fn local<'a>(node: Node<'a, '_>) -> &'a str {
    node.tag_name().name()
}

impl Context<'_> {
    fn warn(&mut self, text: String) {
        if !self.warnings.contains(&text) {
            self.warnings.push(text);
        }
    }

    fn qname(&mut self, node: Node<'_, '_>, value: &str) -> Option<QName> {
        match resolve_qname(node, value) {
            Ok(name) => Some(name),
            Err(error) => {
                self.warn(format!("Schema: {}", error.message));
                None
            }
        }
    }

    /// False, with a warning, for a name that is not an NCName (names become tags).
    fn valid_name(&mut self, name: &str) -> bool {
        if is_ncname(name) {
            return true;
        }
        let shown: String = name.chars().take(60).collect();
        self.warn(format!(
            "Schema: \"{}\" is not a valid XML name and was skipped.",
            neutralise(&shown)
        ));
        false
    }

    fn unsupported(&mut self, node: Node<'_, '_>) {
        self.warn(format!(
            "Schema: <{}> is not supported and was skipped.",
            local(node)
        ));
    }

    fn qualified(&self, node: Node<'_, '_>, default: bool) -> bool {
        match node.attribute("form") {
            Some("qualified") => true,
            Some("unqualified") => false,
            _ => default,
        }
    }
}

/// Reads one `xs:schema` element into `set`. `chameleon` is the namespace of the schema
/// that included this one, used when it declares none.
pub fn read_schema(
    schema: Node<'_, '_>,
    chameleon: Option<&str>,
    set: &mut SchemaSet,
    warnings: &mut Vec<String>,
) {
    let tns = schema
        .attribute("targetNamespace")
        .or(chameleon)
        .unwrap_or_default()
        .to_string();
    let mut context = Context {
        tns,
        elements_qualified: schema.attribute("elementFormDefault") == Some("qualified"),
        attributes_qualified: schema.attribute("attributeFormDefault") == Some("qualified"),
        warnings,
    };
    for child in elements(schema) {
        read_global(&mut context, child, set);
    }
}

fn read_global(context: &mut Context<'_>, node: Node<'_, '_>, set: &mut SchemaSet) {
    if node.tag_name().namespace() != Some(XSD_NS) {
        return context.unsupported(node);
    }
    let Some(name) = node.attribute("name") else {
        return match local(node) {
            "import" | "include" | "annotation" | "notation" => {}
            "redefine" | "override" => context.unsupported(node),
            _ => context.warn(format!("Schema: a global <{}> has no name.", local(node))),
        };
    };
    if !context.valid_name(name) {
        return;
    }
    let key = QName::new(context.tns.clone(), name).key();
    match local(node) {
        "element" => {
            let decl = element_decl(context, node, name, context.tns.clone(), Occurs::default());
            set.elements.insert(key, decl);
        }
        "complexType" => {
            let def = complex_type(context, node, name);
            set.types.insert(key, TypeDef::Complex { def });
        }
        "simpleType" => {
            let def = simple_type(context, node);
            set.types.insert(key, TypeDef::Simple { def });
        }
        "group" => {
            if let Some(particle) = elements(node).find_map(|child| group_particle(context, child))
            {
                set.groups.insert(key, particle);
            }
        }
        "attributeGroup" => {
            let attributes = attributes_of(context, node);
            set.attribute_groups.insert(key, attributes);
        }
        "attribute" | "notation" | "annotation" => {}
        _ => context.unsupported(node),
    }
}

fn occurs(node: Node<'_, '_>) -> Occurs {
    let min = node
        .attribute("minOccurs")
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(1);
    let max = match node.attribute("maxOccurs").map(str::trim) {
        Some("unbounded") => None,
        Some(value) => Some(value.parse().unwrap_or(1)),
        None => Some(1),
    };
    Occurs { min, max }
}

fn element_decl(
    context: &mut Context<'_>,
    node: Node<'_, '_>,
    name: &str,
    ns: String,
    occurs: Occurs,
) -> ElementDecl {
    let type_ref = match node.attribute("type") {
        Some(value) => context
            .qname(node, value)
            .map_or(TypeRef::Any, |name| TypeRef::Named { name }),
        None => inline_type(context, node, name),
    };
    ElementDecl {
        name: name.to_string(),
        ns,
        type_ref,
        occurs,
    }
}

fn inline_type(context: &mut Context<'_>, node: Node<'_, '_>, label: &str) -> TypeRef {
    let found = elements(node).find_map(|child| match local(child) {
        "complexType" if is_xsd(child, "complexType") => Some(TypeDef::Complex {
            def: complex_type(context, child, label),
        }),
        "simpleType" if is_xsd(child, "simpleType") => Some(TypeDef::Simple {
            def: simple_type(context, child),
        }),
        _ => None,
    });
    found.map_or(TypeRef::Any, |def| TypeRef::Inline { def: Box::new(def) })
}

fn local_element(context: &mut Context<'_>, node: Node<'_, '_>) -> Option<Particle> {
    let occurs = occurs(node);
    if let Some(value) = node.attribute("ref") {
        let name = context.qname(node, value)?;
        return Some(Particle::ElementRef { name, occurs });
    }
    let Some(name) = node.attribute("name") else {
        context.warn("Schema: a local <element> has neither name nor ref.".to_string());
        return None;
    };
    if !context.valid_name(name) {
        return None;
    }
    let ns = if context.qualified(node, context.elements_qualified) {
        context.tns.clone()
    } else {
        String::new()
    };
    let decl = element_decl(context, node, name, ns, occurs);
    Some(Particle::Element { decl })
}

/// A particle child of a compositor or type: element, nested group, group ref or any.
fn group_particle(context: &mut Context<'_>, node: Node<'_, '_>) -> Option<Particle> {
    if node.tag_name().namespace() != Some(XSD_NS) {
        context.unsupported(node);
        return None;
    }
    let compositor = match local(node) {
        "sequence" => Compositor::Sequence,
        "all" => Compositor::All,
        "choice" => Compositor::Choice,
        "element" => return local_element(context, node),
        "any" => {
            return Some(Particle::Any {
                occurs: occurs(node),
            })
        }
        "group" => {
            let name = context.qname(node, node.attribute("ref")?)?;
            return Some(Particle::GroupRef {
                name,
                occurs: occurs(node),
            });
        }
        "annotation" => return None,
        _ => {
            context.unsupported(node);
            return None;
        }
    };
    let items = elements(node)
        .filter_map(|child| group_particle(context, child))
        .collect();
    Some(Particle::Group {
        compositor,
        items,
        occurs: occurs(node),
    })
}

fn attribute_use(context: &mut Context<'_>, node: Node<'_, '_>) -> Option<AttributeUse> {
    if node.attribute("use") == Some("prohibited") {
        return None;
    }
    let required = node.attribute("use") == Some("required");
    if let Some(value) = node.attribute("ref") {
        let name = context.qname(node, value)?;
        return Some(AttributeUse::Attribute {
            name: name.name,
            ns: name.ns,
            required,
        });
    }
    let name = node.attribute("name")?.to_string();
    if !context.valid_name(&name) {
        return None;
    }
    let ns = if context.qualified(node, context.attributes_qualified) {
        context.tns.clone()
    } else {
        String::new()
    };
    Some(AttributeUse::Attribute { name, ns, required })
}

/// The `attribute` and `attributeGroup` children of `node`.
fn attributes_of(context: &mut Context<'_>, node: Node<'_, '_>) -> Vec<AttributeUse> {
    elements(node)
        .filter_map(|child| match local(child) {
            "attribute" if is_xsd(child, "attribute") => attribute_use(context, child),
            "attributeGroup" if is_xsd(child, "attributeGroup") => {
                let name = context.qname(child, child.attribute("ref")?)?;
                Some(AttributeUse::GroupRef { name })
            }
            _ => None,
        })
        .collect()
}

/// The content particle of `node` (the first compositor or group ref child).
fn content_of(context: &mut Context<'_>, node: Node<'_, '_>) -> Option<Particle> {
    elements(node)
        .filter(|child| {
            matches!(local(*child), "sequence" | "all" | "choice" | "group")
                && child.tag_name().namespace() == Some(XSD_NS)
        })
        .find_map(|child| group_particle(context, child))
}

fn derived(
    context: &mut Context<'_>,
    content: Node<'_, '_>,
    label: &str,
    simple: bool,
) -> ComplexType {
    let Some(step) =
        elements(content).find(|c| is_xsd(*c, "extension") || is_xsd(*c, "restriction"))
    else {
        context.unsupported(content);
        return ComplexType::empty(label);
    };
    let derivation = if local(step) == "extension" {
        Derivation::Extension
    } else {
        Derivation::Restriction
    };
    let base = step
        .attribute("base")
        .and_then(|value| context.qname(step, value))
        .map(|name| (name, derivation));
    ComplexType {
        label: label.to_string(),
        base,
        content: if simple {
            None
        } else {
            content_of(context, step)
        },
        attributes: attributes_of(context, step),
        simple,
    }
}

fn complex_type(context: &mut Context<'_>, node: Node<'_, '_>, label: &str) -> ComplexType {
    if let Some(content) = elements(node).find(|child| is_xsd(*child, "complexContent")) {
        return derived(context, content, label, false);
    }
    if let Some(content) = elements(node).find(|child| is_xsd(*child, "simpleContent")) {
        return derived(context, content, label, true);
    }
    ComplexType {
        label: label.to_string(),
        base: None,
        content: content_of(context, node),
        attributes: attributes_of(context, node),
        simple: false,
    }
}

fn simple_type(context: &mut Context<'_>, node: Node<'_, '_>) -> SimpleType {
    let Some(step) = elements(node).find(|child| {
        child.tag_name().namespace() == Some(XSD_NS) && local(*child) != "annotation"
    }) else {
        return SimpleType::default();
    };
    match local(step) {
        "restriction" => SimpleType {
            enumeration: elements(step)
                .filter(|facet| is_xsd(*facet, "enumeration"))
                .filter_map(|facet| facet.attribute("value"))
                .map(str::to_string)
                .collect(),
            base: step
                .attribute("base")
                .and_then(|value| context.qname(step, value)),
        },
        "union" => SimpleType {
            enumeration: Vec::new(),
            base: step
                .attribute("memberTypes")
                .and_then(|types| types.split_whitespace().next())
                .and_then(|first| context.qname(step, first)),
        },
        "list" => SimpleType::default(),
        _ => {
            context.unsupported(step);
            SimpleType::default()
        }
    }
}

impl ComplexType {
    fn empty(label: &str) -> Self {
        Self {
            label: label.to_string(),
            base: None,
            content: None,
            attributes: Vec::new(),
            simple: false,
        }
    }
}

#[cfg(test)]
#[path = "xsd_tests.rs"]
mod tests;
