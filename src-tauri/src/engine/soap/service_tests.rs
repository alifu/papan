use std::path::Path;

use super::*;
use crate::engine::soap::test_support::fixture;
use crate::engine::soap::walk::{Fetched, Get};
use crate::store::types::SoapVersion;

/// Serves the calculator for any URL and files from the fixtures.
struct Calculator;

impl Fetch for Calculator {
    async fn get(&self, request: Get<'_>) -> Result<Fetched, AppError> {
        Ok(Fetched::Document {
            url: request.url.clone(),
            bytes: fixture("calculator.wsdl"),
        })
    }

    async fn read(&self, path: &Path, _: &Path, _: usize) -> Result<Vec<u8>, AppError> {
        Ok(fixture(&path.file_name().unwrap().to_string_lossy()))
    }
}

fn plain(text: &str) -> String {
    text.replace("s3cret", "[REDACTED]")
}

#[test]
fn without_credentials_drops_userinfo_even_in_templates() {
    assert_eq!(
        without_credentials("http://u:p@{{host}}/a?x=@"),
        "http://{{host}}/a?x=@"
    );
    assert_eq!(without_credentials("https://h/a@b"), "https://h/a@b");
    assert_eq!(without_credentials("{{url}}"), "{{url}}");
}

#[tokio::test]
async fn a_url_import_is_cached_and_read_back_by_its_reference() {
    let dir = tempfile::tempdir().unwrap();
    let service = WsdlService::new(dir.path());
    let typed = "http://u:pw@example.test/calc.asmx?wsdl&key=s3cret";
    let root = ImportRoot::Url {
        typed: typed.into(),
        url: Url::parse(typed).unwrap(),
    };

    let imported = service
        .import(&Calculator, "scratch:a", root, &plain)
        .await
        .unwrap();
    let reference = WsdlRef {
        url: imported.ref_url.clone(),
        ..WsdlRef::default()
    };
    let cached = service.cached("scratch:a", &reference).unwrap();
    let other_scope = service.cached("scratch:b", &reference).unwrap();

    assert_eq!(
        imported.source,
        "http://example.test/calc.asmx?wsdl&key=[REDACTED]"
    );
    assert_eq!(imported.fetched, vec!["http://example.test/calc.asmx"]);
    assert_eq!(imported.services[0].ports[1].version, SoapVersion::V12);
    assert_eq!(
        imported.services[0].ports[0].operations[0].action,
        "http://tempuri.org/Add"
    );
    assert_eq!(cached, Some(imported.clone()));
    assert_eq!(other_scope, None);

    let template = service
        .envelope(
            &imported.id,
            "Calculator",
            "CalculatorSoap",
            "Add",
            EnvelopeOptions::default(),
        )
        .unwrap();
    assert!(template.envelope.contains("<ns1:Add>"));
}

#[tokio::test]
async fn a_file_import_is_found_by_file_name_and_never_shows_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let service = WsdlService::new(dir.path());
    let root = ImportRoot::File {
        path: PathBuf::from("/somewhere/private/calculator.wsdl"),
    };

    let imported = service
        .import(&Calculator, "collection:c", root, &plain)
        .await
        .unwrap();
    let reference = WsdlRef {
        file: "calculator.wsdl".into(),
        ..WsdlRef::default()
    };

    assert_eq!(imported.source, "calculator.wsdl");
    assert_eq!(imported.ref_url, "");
    assert_eq!(imported.fetched, vec!["calculator.wsdl"]);
    assert_eq!(
        service.cached("collection:c", &reference).unwrap(),
        Some(imported)
    );
    assert_eq!(
        service.cached("collection:c", &WsdlRef::default()).unwrap(),
        None
    );
}

#[test]
fn an_envelope_needs_a_valid_cached_id() {
    let dir = tempfile::tempdir().unwrap();
    let service = WsdlService::new(dir.path());
    let options = EnvelopeOptions::default();

    let bad = service
        .envelope("../../x", "S", "P", "O", options)
        .unwrap_err();
    let gone = service
        .envelope(&"0".repeat(64), "S", "P", "O", options)
        .unwrap_err();

    assert_eq!(bad.code, crate::error::ErrorCode::InvalidInput);
    assert_eq!(gone.code, crate::error::ErrorCode::NotFound);
}

/// What the frontend saves (`ref_url` as `soap.wsdl.url`) must find the cache again.
#[tokio::test]
async fn a_saved_reference_finds_the_cache_for_urls_with_credentials_or_variables() {
    let dir = tempfile::tempdir().unwrap();
    let service = WsdlService::new(dir.path());
    let cases = [
        (
            "https://u:p@example.test/svc?wsdl",
            "https://example.test/svc?wsdl",
        ),
        ("{{base}}/svc?wsdl", "{{base}}/svc?wsdl"),
    ];

    for (typed, shown) in cases {
        let root = ImportRoot::Url {
            typed: typed.into(),
            url: Url::parse("https://example.test/svc?wsdl").unwrap(),
        };
        let imported = service
            .import(&Calculator, "scratch:a", root, &plain)
            .await
            .unwrap();
        let saved = WsdlRef {
            url: imported.ref_url.clone(),
            ..WsdlRef::default()
        };

        assert_eq!(imported.ref_url, shown);
        assert_eq!(service.cached("scratch:a", &saved).unwrap(), Some(imported));
    }
}
