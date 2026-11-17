use std::collections::HashMap;
use std::sync::Mutex;

use super::*;
use crate::engine::soap::test_support::fixture;

/// URL documents, redirects and files in memory; records every GET with its credentials.
#[derive(Default)]
struct Memory {
    urls: HashMap<String, Fetched>,
    files: HashMap<PathBuf, Vec<u8>>,
    gets: Mutex<Vec<(String, bool)>>,
}

impl Memory {
    fn url(self, url: &str, body: impl Into<Vec<u8>>) -> Self {
        let mut urls = self.urls;
        let parsed = Url::parse(url).unwrap();
        urls.insert(
            url.to_string(),
            Fetched::Document {
                url: parsed,
                bytes: body.into(),
            },
        );
        Self { urls, ..self }
    }

    fn redirect(self, from: &str, to: &str) -> Self {
        let mut urls = self.urls;
        urls.insert(from.to_string(), Fetched::Redirect(Url::parse(to).unwrap()));
        Self { urls, ..self }
    }

    fn file(self, path: &str, body: impl Into<Vec<u8>>) -> Self {
        let mut files = self.files;
        files.insert(PathBuf::from(path), body.into());
        Self { files, ..self }
    }

    fn gets(&self) -> Vec<(String, bool)> {
        self.gets.lock().unwrap().clone()
    }
}

impl Fetch for Memory {
    async fn get(&self, request: Get<'_>) -> Result<Fetched, AppError> {
        self.gets
            .lock()
            .unwrap()
            .push((request.url.to_string(), request.credentials));
        match self.urls.get(request.url.as_str()) {
            Some(Fetched::Document { bytes, .. }) if bytes.len() > request.max_bytes => {
                Err(AppError::invalid_input("too large"))
            }
            Some(found) => Ok(found.clone()),
            None => Err(AppError::not_found("404")),
        }
    }

    async fn read(&self, path: &Path, _root: &Path, _max: usize) -> Result<Vec<u8>, AppError> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| AppError::not_found("no such file"))
    }
}

fn root(url: &str) -> Root {
    Root::Url(Url::parse(url).unwrap())
}

const XS: &str = "http://www.w3.org/2001/XMLSchema";

/// A schema document that imports `locations`.
fn xsd(locations: &[&str]) -> String {
    let imports: String = locations
        .iter()
        .map(|location| format!(r#"<xs:import schemaLocation="{location}"/>"#))
        .collect();
    format!(r#"<xs:schema xmlns:xs="{XS}" targetNamespace="urn:x">{imports}</xs:schema>"#)
}

fn calculator_importing(locations: &[&str]) -> Vec<u8> {
    let text = String::from_utf8(fixture("calculator.wsdl")).unwrap();
    let imports: String = locations
        .iter()
        .map(|location| format!(r#"<xs:import schemaLocation="{location}"/>"#))
        .collect();
    text.replacen(
        r#"<xs:schema elementFormDefault="qualified" targetNamespace="http://tempuri.org/">"#,
        &format!(r#"<xs:schema elementFormDefault="qualified" targetNamespace="http://tempuri.org/">{imports}"#),
        1,
    )
    .into_bytes()
}

const ROOT: &str = "http://example.test/svc/calc.wsdl";

#[tokio::test]
async fn follows_relative_imports_once_each_with_credentials_only_on_the_root_origin() {
    let fetch = Memory::default()
        .url(
            ROOT,
            calculator_importing(&["a.xsd", "http://cdn.test/b.xsd", "a.xsd"]),
        )
        .url(
            "http://example.test/svc/a.xsd",
            xsd(&["../svc/calc.wsdl", "http://cdn.test/b.xsd"]),
        )
        .url("http://cdn.test/b.xsd", xsd(&["a.xsd"]))
        .url("http://cdn.test/a.xsd", xsd(&[]));

    let walked = walk(&fetch, root(ROOT), WalkLimits::default())
        .await
        .unwrap();

    assert_eq!(
        fetch.gets(),
        vec![
            (ROOT.to_string(), true),
            ("http://example.test/svc/a.xsd".to_string(), true),
            ("http://cdn.test/b.xsd".to_string(), false),
            ("http://cdn.test/a.xsd".to_string(), false),
        ]
    );
    assert_eq!(walked.fetched.len(), 4);
    assert!(walked.warnings.is_empty(), "{:?}", walked.warnings);
    assert_eq!(walked.model.services[0].name, "Calculator");
}

#[tokio::test]
async fn split_wsdl_imports_resolve_through_wsdl_and_xsd_and_include() {
    let types = "http://example.test/split/types.xsd";
    let fetch = Memory::default()
        .url(
            "http://example.test/split/main.wsdl",
            fixture("split/main.wsdl"),
        )
        .url(types, fixture("split/types.xsd"))
        .url(
            "http://example.test/split/common.xsd",
            fixture("split/common.xsd"),
        );

    let walked = walk(
        &fetch,
        root("http://example.test/split/main.wsdl"),
        WalkLimits::default(),
    )
    .await
    .unwrap();

    assert_eq!(walked.fetched.len(), 3);
    assert_eq!(walked.model.schema.types.len(), 2);
}

#[tokio::test]
async fn a_file_url_in_a_url_document_is_refused_with_a_warning() {
    let fetch = Memory::default().url(ROOT, calculator_importing(&["file:///etc/passwd"]));

    let walked = walk(&fetch, root(ROOT), WalkLimits::default())
        .await
        .unwrap();

    assert_eq!(fetch.gets().len(), 1);
    assert!(
        walked.warnings[0].contains("only http and https"),
        "{:?}",
        walked.warnings
    );
}

#[tokio::test]
async fn a_private_literal_cross_origin_import_is_refused_unless_the_root_is_private() {
    let lan = "http://192.168.1.5/t.xsd";
    let public = Memory::default()
        .url(ROOT, calculator_importing(&[lan]))
        .url(lan, xsd(&[]));
    let local_root = "http://127.0.0.1:9/calc.wsdl";
    let private = Memory::default()
        .url(local_root, calculator_importing(&[lan]))
        .url(lan, xsd(&[]));

    let refused = walk(&public, root(ROOT), WalkLimits::default())
        .await
        .unwrap();
    let allowed = walk(&private, root(local_root), WalkLimits::default())
        .await
        .unwrap();

    assert!(refused.warnings[0].contains("private address"));
    assert_eq!(public.gets().len(), 1);
    assert_eq!(
        private.gets(),
        vec![(local_root.to_string(), true), (lan.to_string(), false)]
    );
    assert!(allowed.warnings.is_empty());
}

/// The root, then `d1.xsd` … `dN.xsd`, each importing the next; the last imports nothing.
fn chain(length: usize) -> Memory {
    (1..=length).fold(
        Memory::default().url(ROOT, calculator_importing(&["d1.xsd"])),
        |fetch, index| {
            let next = format!("d{}.xsd", index + 1);
            let imports: Vec<&str> = if index < length { vec![&next] } else { vec![] };
            fetch.url(
                &format!("http://example.test/svc/d{index}.xsd"),
                xsd(&imports),
            )
        },
    )
}

#[tokio::test]
async fn import_depth_eight_is_read_and_depth_nine_is_refused() {
    let eight = walk(&chain(8), root(ROOT), WalkLimits::default())
        .await
        .unwrap();
    let nine = walk(&chain(9), root(ROOT), WalkLimits::default()).await;

    assert_eq!(eight.fetched.len(), 9);
    assert!(nine.unwrap_err().message.contains("deeper than 8"));
}

#[tokio::test]
async fn the_fifty_first_document_is_refused() {
    let names: Vec<String> = (0..50).map(|index| format!("x{index}.xsd")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let fetch = names.iter().fold(
        Memory::default().url(ROOT, calculator_importing(&refs)),
        |fetch, name| fetch.url(&format!("http://example.test/svc/{name}"), xsd(&[])),
    );
    let forty_nine = names.iter().take(49).fold(
        Memory::default().url(ROOT, calculator_importing(&refs[..49])),
        |fetch, name| fetch.url(&format!("http://example.test/svc/{name}"), xsd(&[])),
    );

    let error = walk(&fetch, root(ROOT), WalkLimits::default())
        .await
        .unwrap_err();
    let ok = walk(&forty_nine, root(ROOT), WalkLimits::default())
        .await
        .unwrap();

    assert!(error.message.contains("more than 50 documents"));
    assert_eq!(ok.fetched.len(), 50);
}

#[tokio::test]
async fn the_total_size_is_capped() {
    let fetch = Memory::default()
        .url(ROOT, calculator_importing(&["big.xsd"]))
        .url(
            "http://example.test/svc/big.xsd",
            format!("{}<!--{}-->", xsd(&[]), "x".repeat(5000)),
        );
    let limits = WalkLimits {
        total_bytes: 6000,
        ..WalkLimits::default()
    };

    let error = walk(&fetch, root(ROOT), limits).await.unwrap_err();

    assert!(error.message.contains("larger than"));
}

#[tokio::test]
async fn redirects_are_checked_hop_by_hop_and_capped() {
    let to_file = Memory::default().redirect(ROOT, "file:///etc/passwd");
    let hop = |index: usize| format!("http://example.test/r{index}");
    let six = (0..6).fold(Memory::default().redirect(ROOT, &hop(0)), |fetch, index| {
        fetch.redirect(&hop(index), &hop(index + 1))
    });
    let cross = Memory::default()
        .redirect(ROOT, "http://mirror.test/calc.wsdl")
        .url(
            "http://mirror.test/calc.wsdl",
            calculator_importing(&["t.xsd"]),
        )
        .url("http://mirror.test/t.xsd", xsd(&[]));

    let file_error = walk(&to_file, root(ROOT), WalkLimits::default())
        .await
        .unwrap_err();
    let loop_error = walk(&six, root(ROOT), WalkLimits::default())
        .await
        .unwrap_err();
    let walked = walk(&cross, root(ROOT), WalkLimits::default())
        .await
        .unwrap();

    assert!(file_error.message.contains("only http and https"));
    assert!(loop_error.message.contains("more than 5 times"));
    assert_eq!(
        walked.fetched,
        vec!["http://mirror.test/calc.wsdl", "http://mirror.test/t.xsd"]
    );
    assert_eq!(
        cross.gets(),
        vec![
            (ROOT.to_string(), true),
            ("http://mirror.test/calc.wsdl".to_string(), false),
            ("http://mirror.test/t.xsd".to_string(), false),
        ]
    );
}

#[tokio::test]
async fn a_missing_import_is_a_warning_but_a_missing_root_fails() {
    let fetch = Memory::default().url(ROOT, calculator_importing(&["gone.xsd"]));

    let walked = walk(&fetch, root(ROOT), WalkLimits::default())
        .await
        .unwrap();
    let error = walk(&Memory::default(), root(ROOT), WalkLimits::default())
        .await
        .unwrap_err();

    assert!(walked.warnings[0].starts_with("Skipped http://example.test/svc/gone.xsd"));
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[tokio::test]
async fn a_file_root_reads_siblings_and_refuses_escapes_wrong_extensions_and_file_urls() {
    let main = calculator_importing(&[
        "sub/a.xsd",
        "../outside.xsd",
        "notes.txt",
        "file:///etc/hosts",
        "http://10.0.0.1/x.xsd",
        "http://cdn.test/p.xsd",
    ]);
    let fetch = Memory::default()
        .file("/w/calc.wsdl", main)
        .file("/w/sub/a.xsd", xsd(&["b.xsd"]))
        .file("/w/sub/b.xsd", xsd(&[]))
        .url("http://cdn.test/p.xsd", xsd(&[]));

    let walked = walk(
        &fetch,
        Root::File(PathBuf::from("/w/calc.wsdl")),
        WalkLimits::default(),
    )
    .await
    .unwrap();

    assert_eq!(
        walked.fetched,
        vec![
            "calc.wsdl",
            "sub/a.xsd",
            "http://cdn.test/p.xsd",
            "sub/b.xsd"
        ]
    );
    assert_eq!(
        fetch.gets(),
        vec![("http://cdn.test/p.xsd".to_string(), false)]
    );
    assert_eq!(walked.warnings.len(), 4, "{:?}", walked.warnings);
}

#[tokio::test]
async fn a_hostile_document_fails_the_root_and_a_dtd_import_is_skipped() {
    let hostile = Memory::default().url(ROOT, fixture("bad-doctype-xxe.wsdl"));
    let importing = Memory::default()
        .url(ROOT, calculator_importing(&["x.xsd"]))
        .url(
            "http://example.test/svc/x.xsd",
            fixture("bad-billion-laughs.wsdl"),
        );

    let error = walk(&hostile, root(ROOT), WalkLimits::default())
        .await
        .unwrap_err();
    let walked = walk(&importing, root(ROOT), WalkLimits::default())
        .await
        .unwrap();

    assert!(error.message.contains("DTD"));
    assert!(walked.warnings[0].contains("DTD"));
}

/// Never answers, to test the wall clock.
struct Hanging;

impl Fetch for Hanging {
    async fn get(&self, _: Get<'_>) -> Result<Fetched, AppError> {
        std::future::pending().await
    }

    async fn read(&self, _: &Path, _: &Path, _: usize) -> Result<Vec<u8>, AppError> {
        std::future::pending().await
    }
}

#[tokio::test]
async fn the_whole_import_has_a_wall_clock() {
    let limits = WalkLimits {
        wall: Duration::from_millis(20),
        ..WalkLimits::default()
    };

    let error = walk(&Hanging, root(ROOT), limits).await.unwrap_err();

    assert_eq!(error.code, ErrorCode::Timeout);
}

#[tokio::test]
async fn a_non_http_root_is_refused() {
    let error = walk(
        &Memory::default(),
        root("file:///etc/hosts"),
        WalkLimits::default(),
    )
    .await
    .unwrap_err();

    assert!(error.message.contains("only http and https"));
}

#[tokio::test]
async fn a_document_with_variable_braces_is_reported() {
    let text = String::from_utf8(calculator_importing(&[])).unwrap();
    let planted = text.replacen("http://tempuri.org/Add\"", "{{token}}\"", 1);
    let fetch = Memory::default().url(ROOT, planted);

    let walked = walk(&fetch, root(ROOT), WalkLimits::default())
        .await
        .unwrap();

    assert!(
        walked.warnings.iter().any(|w| w.contains("{{")),
        "{:?}",
        walked.warnings
    );
}

#[tokio::test]
async fn fetched_locations_carry_no_query_fragment_or_credentials() {
    let typed = "http://u:pw@example.test/svc/calc.wsdl?wsdl&key=k3y#part";
    let fetch = Memory::default().url(typed, calculator_importing(&["t.xsd?v=s3cret"]));
    // A relative import keeps the base's user info (same origin).
    let fetch = fetch.url("http://u:pw@example.test/svc/t.xsd?v=s3cret", xsd(&[]));

    let walked = walk(&fetch, root(typed), WalkLimits::default())
        .await
        .unwrap();

    assert_eq!(
        walked.fetched,
        vec![
            "http://example.test/svc/calc.wsdl",
            "http://example.test/svc/t.xsd"
        ]
    );
}
