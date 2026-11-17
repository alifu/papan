use super::*;
use crate::engine::soap::xml::{parse, XmlLimits};

const TNS: &str = "urn:t";

fn read(body: &str) -> (SchemaSet, Vec<String>) {
    read_with(
        &format!(
            r#"<xs:schema xmlns:xs="{XSD_NS}" xmlns:t="{TNS}" targetNamespace="{TNS}">{body}</xs:schema>"#
        ),
        None,
    )
}

fn read_with(text: &str, chameleon: Option<&str>) -> (SchemaSet, Vec<String>) {
    let document = parse(text, XmlLimits::default()).unwrap();
    let mut set = SchemaSet::default();
    let mut warnings = Vec::new();
    read_schema(document.root_element(), chameleon, &mut set, &mut warnings);
    (set, warnings)
}

fn key(name: &str) -> String {
    QName::new(TNS, name).key()
}

fn complex(set: &SchemaSet, name: &str) -> ComplexType {
    match set.types.get(&key(name)) {
        Some(TypeDef::Complex { def }) => def.clone(),
        other => panic!("{name}: {other:?}"),
    }
}

fn group_items(particle: &Option<Particle>) -> Vec<Particle> {
    match particle {
        Some(Particle::Group { items, .. }) => items.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn reads_global_elements_with_inline_and_named_types() {
    let (set, warnings) = read(
        r#"<xs:element name="Add"><xs:complexType><xs:sequence>
             <xs:element name="a" type="xs:int"/>
           </xs:sequence></xs:complexType></xs:element>
           <xs:element name="Plain" type="t:Thing"/>
           <xs:element name="Untyped"/>"#,
    );

    let add = set.elements.get(&key("Add")).unwrap();
    let plain = set.elements.get(&key("Plain")).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(add.ns, TNS);
    assert!(matches!(&add.type_ref, TypeRef::Inline { .. }));
    assert_eq!(
        plain.type_ref,
        TypeRef::Named {
            name: QName::new(TNS, "Thing")
        }
    );
    assert_eq!(
        set.elements.get(&key("Untyped")).unwrap().type_ref,
        TypeRef::Any
    );
}

#[test]
fn local_elements_follow_element_form_default_and_form() {
    let text = format!(
        r#"<xs:schema xmlns:xs="{XSD_NS}" targetNamespace="{TNS}" elementFormDefault="qualified">
             <xs:complexType name="T"><xs:sequence>
               <xs:element name="q" type="xs:string"/>
               <xs:element name="u" form="unqualified" type="xs:string"/>
             </xs:sequence></xs:complexType></xs:schema>"#
    );
    let (set, _) = read_with(&text, None);
    let (plain, _) = read(
        r#"<xs:complexType name="T"><xs:sequence>
             <xs:element name="u" type="xs:string"/>
             <xs:element name="q" form="qualified" type="xs:string"/>
           </xs:sequence></xs:complexType>"#,
    );

    let ns_of = |set: &SchemaSet| -> Vec<(String, String)> {
        group_items(&complex(set, "T").content)
            .into_iter()
            .map(|particle| match particle {
                Particle::Element { decl } => (decl.name, decl.ns),
                other => panic!("{other:?}"),
            })
            .collect()
    };
    assert_eq!(
        ns_of(&set),
        vec![("q".into(), TNS.into()), ("u".into(), String::new())]
    );
    assert_eq!(
        ns_of(&plain),
        vec![("u".into(), String::new()), ("q".into(), TNS.into())]
    );
}

#[test]
fn reads_occurrences_refs_choice_any_and_groups() {
    let (set, warnings) = read(
        r#"<xs:group name="G"><xs:sequence><xs:element name="g" type="xs:string"/></xs:sequence></xs:group>
           <xs:complexType name="T"><xs:sequence>
             <xs:element ref="t:Other" minOccurs="0"/>
             <xs:choice><xs:element name="a"/><xs:element name="b"/></xs:choice>
             <xs:element name="many" maxOccurs="unbounded" minOccurs="0"/>
             <xs:element name="three" maxOccurs="3"/>
             <xs:group ref="t:G"/>
             <xs:any minOccurs="0"/>
           </xs:sequence></xs:complexType>"#,
    );

    let items = group_items(&complex(&set, "T").content);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(set.groups.contains_key(&key("G")));
    assert_eq!(
        items[0],
        Particle::ElementRef {
            name: QName::new(TNS, "Other"),
            occurs: Occurs {
                min: 0,
                max: Some(1)
            }
        }
    );
    assert!(matches!(
        &items[1],
        Particle::Group { compositor: Compositor::Choice, items, .. } if items.len() == 2
    ));
    assert!(
        matches!(&items[2], Particle::Element { decl } if decl.occurs == Occurs { min: 0, max: None })
    );
    assert!(matches!(&items[3], Particle::Element { decl } if decl.occurs.max == Some(3)));
    assert!(matches!(&items[4], Particle::GroupRef { .. }));
    assert!(matches!(&items[5], Particle::Any { .. }));
}

#[test]
fn reads_extension_restriction_simple_content_and_attributes() {
    let (set, _) = read(
        r#"<xs:attributeGroup name="AG"><xs:attribute name="g"/></xs:attributeGroup>
           <xs:complexType name="Base"><xs:sequence><xs:element name="a"/></xs:sequence>
             <xs:attribute name="id" use="required"/><xs:attribute name="gone" use="prohibited"/>
             <xs:attributeGroup ref="t:AG"/><xs:anyAttribute/></xs:complexType>
           <xs:complexType name="Ext"><xs:complexContent><xs:extension base="t:Base">
             <xs:sequence><xs:element name="b"/></xs:sequence><xs:attribute name="x"/>
           </xs:extension></xs:complexContent></xs:complexType>
           <xs:complexType name="Res"><xs:complexContent><xs:restriction base="xs:anyType">
             <xs:sequence><xs:element name="c"/></xs:sequence></xs:restriction></xs:complexContent></xs:complexType>
           <xs:complexType name="Text"><xs:simpleContent><xs:extension base="xs:string">
             <xs:attribute name="lang"/></xs:extension></xs:simpleContent></xs:complexType>"#,
    );

    let base = complex(&set, "Base");
    let ext = complex(&set, "Ext");
    let res = complex(&set, "Res");
    let text = complex(&set, "Text");
    assert_eq!(base.attributes.len(), 2);
    assert_eq!(
        base.attributes[0],
        AttributeUse::Attribute {
            name: "id".into(),
            ns: String::new(),
            required: true
        }
    );
    assert!(set.attribute_groups.contains_key(&key("AG")));
    assert_eq!(
        ext.base,
        Some((QName::new(TNS, "Base"), Derivation::Extension))
    );
    assert_eq!(group_items(&ext.content).len(), 1);
    assert_eq!(res.base.map(|(_, how)| how), Some(Derivation::Restriction));
    assert!(text.simple && text.content.is_none() && text.attributes.len() == 1);
}

#[test]
fn reads_simple_types_enumeration_union_and_list() {
    let (set, _) = read(
        r#"<xs:simpleType name="E"><xs:restriction base="xs:string">
             <xs:enumeration value="a"/><xs:enumeration value="b"/></xs:restriction></xs:simpleType>
           <xs:simpleType name="U"><xs:union memberTypes="xs:int xs:string"/></xs:simpleType>
           <xs:simpleType name="L"><xs:list itemType="xs:int"/></xs:simpleType>"#,
    );

    let simple = |name: &str| match set.types.get(&key(name)) {
        Some(TypeDef::Simple { def }) => def.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(simple("E").enumeration, vec!["a", "b"]);
    assert_eq!(simple("U").base, Some(QName::new(XSD_NS, "int")));
    assert_eq!(simple("L"), SimpleType::default());
}

#[test]
fn unknown_constructs_become_warnings() {
    let (_, warnings) = read(
        r#"<xs:redefine schemaLocation="x.xsd"/><xs:element/>
           <xs:complexType name="T"><xs:sequence><xs:element/><xs:weird/></xs:sequence></xs:complexType>
           <xs:element name="Bad" type="nope:X"/>"#,
    );

    assert_eq!(warnings.len(), 5, "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("<redefine>")));
    assert!(warnings.iter().any(|w| w.contains("<weird>")));
    assert!(warnings.iter().any(|w| w.contains("not declared")));
}

#[test]
fn a_schema_without_a_target_namespace_takes_the_includers() {
    let text = format!(r#"<xs:schema xmlns:xs="{XSD_NS}"><xs:element name="E"/></xs:schema>"#);

    let (set, _) = read_with(&text, Some(TNS));

    assert!(set.elements.contains_key(&key("E")));
}
