use super::*;
use crate::engine::soap::test_support::fixture;
use crate::error::ErrorCode;

fn parse_bytes(bytes: &[u8]) -> Result<String, AppError> {
    let text = decode(bytes, MAX_DOCUMENT_BYTES)?;
    let document = parse(&text, XmlLimits::default())?;
    Ok(document.root_element().tag_name().name().to_string())
}

#[test]
fn reads_a_utf8_wsdl() {
    assert_eq!(
        parse_bytes(&fixture("calculator.wsdl")).unwrap(),
        "definitions"
    );
}

#[test]
fn refuses_a_doctype_with_an_external_entity() {
    let error = parse_bytes(&fixture("bad-doctype-xxe.wsdl")).unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert_eq!(error.message, DTD_REFUSED);
}

#[test]
fn refuses_an_entity_expansion_chain() {
    let error = parse_bytes(&fixture("bad-billion-laughs.wsdl")).unwrap_err();

    assert_eq!(error.message, DTD_REFUSED);
}

#[test]
fn refuses_an_empty_doctype_too() {
    let error = parse_bytes(b"<!-- x --><?pi?>\n<!doctype a><a/>").unwrap_err();

    assert_eq!(error.message, DTD_REFUSED);
}

#[test]
fn a_doctype_inside_a_comment_is_not_a_doctype() {
    assert_eq!(parse_bytes(b"<!-- <!DOCTYPE a> --><a/>").unwrap(), "a");
}

#[test]
fn refuses_a_document_over_the_size_cap_before_decoding() {
    let error = decode(&[b' '; 11], 10).unwrap_err();
    let parsed = parse(
        "<a/>      ",
        XmlLimits {
            max_bytes: 5,
            nodes_limit: 10,
        },
    )
    .unwrap_err();

    assert!(error.message.contains("larger than"));
    assert!(parsed.message.contains("larger than"));
}

#[test]
fn refuses_a_document_over_the_node_limit() {
    let text = format!("<a>{}</a>", "<b/>".repeat(20));

    let error = parse(
        &text,
        XmlLimits {
            max_bytes: 1000,
            nodes_limit: 10,
        },
    )
    .unwrap_err();

    assert_eq!(error.message, "The document has too many XML nodes.");
}

#[test]
fn reports_malformed_xml() {
    let error = parse_bytes(b"<a><b></a>").unwrap_err();

    assert_eq!(error.message, "The document is not well-formed XML.");
    assert!(error.detail.is_some());
}

fn utf16(text: &str, big_endian: bool, bom: bool) -> Vec<u8> {
    let units = text.encode_utf16();
    let bytes = units.flat_map(|unit| {
        if big_endian {
            unit.to_be_bytes()
        } else {
            unit.to_le_bytes()
        }
    });
    let mark: &[u8] = match (bom, big_endian) {
        (false, _) => &[],
        (true, true) => &[0xFE, 0xFF],
        (true, false) => &[0xFF, 0xFE],
    };
    mark.iter().copied().chain(bytes).collect()
}

#[test]
fn reads_utf16_with_and_without_a_bom() {
    let text = "<?xml version=\"1.0\" encoding=\"UTF-16\"?><définitions/>";

    for (big_endian, bom) in [(true, true), (false, true), (true, false), (false, false)] {
        assert_eq!(
            parse_bytes(&utf16(text, big_endian, bom)).unwrap(),
            "définitions"
        );
    }
}

#[test]
fn reads_a_utf8_bom_and_a_declared_latin1_document() {
    let mut with_bom = vec![0xEF, 0xBB, 0xBF];
    with_bom.extend_from_slice(b"<a/>");
    let latin1 = b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><caf\xe9/>";

    assert_eq!(parse_bytes(&with_bom).unwrap(), "a");
    assert_eq!(parse_bytes(latin1).unwrap(), "café");
}

#[test]
fn a_utf16_label_on_an_ascii_file_is_read_as_utf8() {
    assert_eq!(
        parse_bytes(b"<?xml version='1.0' encoding='utf-16'?><a/>").unwrap(),
        "a"
    );
}

#[test]
fn refuses_invalid_utf8() {
    let error = parse_bytes(b"<a>\xff</a>").unwrap_err();

    assert_eq!(error.message, "The document is not valid UTF-8.");
}

#[test]
fn resolves_qnames_with_the_namespaces_in_scope() {
    let text = r#"<a xmlns="urn:default" xmlns:t="urn:t"><b xmlns:t="urn:inner" x="t:Name"/></a>"#;
    let document = parse(text, XmlLimits::default()).unwrap();
    let b = elements(document.root_element()).next().unwrap();

    assert_eq!(
        resolve_qname(b, "t:Name").unwrap(),
        QName::new("urn:inner", "Name")
    );
    assert_eq!(
        resolve_qname(b, " Plain ").unwrap(),
        QName::new("urn:default", "Plain")
    );
    assert!(resolve_qname(b, "nope:Name").is_err());
    assert!(resolve_qname(b, "t:").is_err());
    assert_eq!(
        child_named(document.root_element(), "urn:default", "b").map(|n| n.attribute("x")),
        Some(Some("t:Name"))
    );
    assert!(required_name(b, "message").is_err());
}

#[test]
fn an_unprefixed_qname_without_a_default_namespace_has_no_namespace() {
    let document = parse("<a/>", XmlLimits::default()).unwrap();

    assert_eq!(
        resolve_qname(document.root_element(), "Name").unwrap(),
        QName::new("", "Name")
    );
}

#[test]
fn only_ncnames_are_accepted_as_names() {
    for good in ["a", "_x", "Add", "int-A.b_9", "définition"] {
        assert!(is_ncname(good), "{good}");
    }
    for bad in [
        "", "1a", "-a", "a b", "a:b", "z><inj", "a\"", "{{x}}", "a&b", "a'",
    ] {
        assert!(!is_ncname(bad), "{bad}");
    }
    let document = parse(
        r#"<a xmlns:t="urn:t" name="x&lt;y"/>"#,
        XmlLimits::default(),
    )
    .unwrap();
    let root = document.root_element();
    assert!(resolve_qname(root, "t:a>b").is_err());
    assert!(required_name(root, "message")
        .unwrap_err()
        .message
        .contains("not a valid XML name"));
}
