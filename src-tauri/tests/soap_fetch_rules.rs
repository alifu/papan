//! The fetch rules of plan 06 §7.3 🔒, one test per row, over the real engine where a
//! local server can show it and over an in-memory `Fetch` where a public host is needed.

mod support;

use std::path::Path;
use std::time::Duration;

use papan_lib::engine::http::HttpEngine;
use papan_lib::engine::soap::fetch::EngineFetch;
use papan_lib::engine::soap::service::{ImportRoot, WsdlService};
use papan_lib::engine::soap::walk::{Fetch, Fetched, Get, WalkLimits};
use papan_lib::error::{AppError, ErrorCode};
use papan_lib::store::types::{Auth, RequestLocation};
use support::wsdl_server::{self, Docs, PASSWORD, USER};
use url::Url;

const SCOPE: &str = "collection:c";

fn fixture_text(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/wsdl")
            .join(name),
    )
    .unwrap()
}

/// The calculator WSDL with schema imports of `locations`.
fn calculator_importing(locations: &[&str]) -> String {
    let imports: String = locations
        .iter()
        .map(|location| format!(r#"<xs:import schemaLocation="{location}"/>"#))
        .collect();
    let open =
        r#"<xs:schema elementFormDefault="qualified" targetNamespace="http://tempuri.org/">"#;
    fixture_text("calculator.wsdl").replacen(open, &format!("{open}{imports}"), 1)
}

const XSD: &str =
    r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:x"/>"#;

struct Setup {
    _dir: tempfile::TempDir,
    engine: HttpEngine,
    service: WsdlService,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let engine = HttpEngine::new(&dir.path().join("cache"), dir.path(), "9.9.9").unwrap();
    let service = WsdlService::new(dir.path());
    Setup {
        _dir: dir,
        engine,
        service,
    }
}

impl Setup {
    async fn import(
        &self,
        root: ImportRoot,
        auth: Auth,
    ) -> Result<papan_lib::engine::soap::WsdlImport, AppError> {
        let fetch = EngineFetch {
            engine: &self.engine,
            request_id: uuid::Uuid::new_v4().to_string(),
            location: RequestLocation::Collection {
                collection_id: "c".into(),
                path: "requests/a.http.yaml".into(),
            },
            auth,
        };
        self.service
            .import(&fetch, SCOPE, root, &|text: &str| {
                text.replace("s3cret", "[REDACTED]")
            })
            .await
    }
}

fn url_root(url: &str) -> ImportRoot {
    ImportRoot::Url {
        typed: url.into(),
        url: Url::parse(url).unwrap(),
    }
}

fn basic() -> Auth {
    Auth::Basic {
        username: USER.into(),
        password: PASSWORD.into(),
    }
}

#[tokio::test]
async fn root_source_only_http_urls_and_no_file_url_from_a_document() {
    let server = wsdl_server::spawn(
        Docs::default().add("/calc.wsdl", calculator_importing(&["file:///etc/hosts"])),
    )
    .await;
    let s = setup();

    let refused = s
        .import(url_root("file:///etc/hosts"), Auth::None)
        .await
        .unwrap_err();
    let imported = s
        .import(url_root(&server.url("/calc.wsdl")), Auth::None)
        .await
        .unwrap();

    assert!(refused.message.contains("only http and https"));
    assert_eq!(imported.fetched.len(), 1);
    assert!(
        imported.warnings[0].contains("only http and https"),
        "{:?}",
        imported.warnings
    );
}

#[tokio::test]
async fn root_auth_goes_to_the_root_origin_only_also_across_redirects() {
    let other =
        wsdl_server::spawn(Docs::default().add("/calc.wsdl", calculator_importing(&[]))).await;
    let root = wsdl_server::spawn(Docs::default().add("/secure/types.xsd", XSD)).await;
    let redirect = root.url(&format!("/redirect?to={}", other.url("/calc.wsdl")));
    let s = setup();

    s.import(url_root(&redirect), basic()).await.unwrap();

    assert!(root.hits()[0].authorization.is_some());
    assert_eq!(other.hits().len(), 1);
    assert_eq!(other.hits()[0].authorization, None);
}

/// A public host in memory: a public root cannot steer an import into the LAN.
struct PublicRoot;

impl Fetch for PublicRoot {
    async fn get(&self, request: Get<'_>) -> Result<Fetched, AppError> {
        let body = match request.url.host_str() {
            Some("example.test") => calculator_importing(&[
                "http://127.0.0.1:1/x.xsd",
                "http://[::1]:1/y.xsd",
                "http://10.0.0.8/z.xsd",
            ]),
            _ => return Err(AppError::internal("a private address was fetched")),
        };
        Ok(Fetched::Document {
            url: request.url.clone(),
            bytes: body.into_bytes(),
        })
    }

    async fn read(&self, _: &Path, _: &Path, _: usize) -> Result<Vec<u8>, AppError> {
        Err(AppError::internal("no files"))
    }
}

#[tokio::test]
async fn imports_from_a_url_root_private_literals_refused_unless_the_root_is_private() {
    let s = setup();
    let lan = wsdl_server::spawn(Docs::default().add("/t.xsd", XSD)).await;
    let local_root = wsdl_server::spawn(
        Docs::default().add("/calc.wsdl", calculator_importing(&[&lan.url("/t.xsd")])),
    )
    .await;

    let public = s
        .service
        .import(
            &PublicRoot,
            SCOPE,
            url_root("http://example.test/calc.wsdl"),
            &|t: &str| t.to_string(),
        )
        .await
        .unwrap();
    let private = s
        .import(url_root(&local_root.url("/calc.wsdl")), Auth::None)
        .await
        .unwrap();

    assert_eq!(public.warnings.len(), 3, "{:?}", public.warnings);
    assert!(public
        .warnings
        .iter()
        .all(|w| w.contains("private address")));
    assert_eq!(private.fetched.len(), 2);
    assert_eq!(lan.hits().len(), 1);
}

#[tokio::test]
async fn imports_from_a_file_root_stay_in_its_folder() {
    let s = setup();
    let outside = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.xsd"), XSD).unwrap();
    std::fs::write(folder.path().join("ok.xsd"), XSD).unwrap();
    std::fs::write(folder.path().join("notes.txt"), XSD).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        outside.path().join("secret.xsd"),
        folder.path().join("link.xsd"),
    )
    .unwrap();
    let escape = format!(
        "../{}/secret.xsd",
        outside.path().file_name().unwrap().to_string_lossy()
    );
    let imports = [
        "ok.xsd",
        escape.as_str(),
        "link.xsd",
        "notes.txt",
        "/etc/hosts.xsd",
        "http://127.0.0.1:1/p.xsd",
    ];
    std::fs::write(
        folder.path().join("main.wsdl"),
        calculator_importing(&imports),
    )
    .unwrap();
    let path = std::fs::canonicalize(folder.path().join("main.wsdl")).unwrap();

    let imported = s
        .import(ImportRoot::File { path }, Auth::None)
        .await
        .unwrap();

    assert_eq!(imported.fetched, vec!["main.wsdl", "ok.xsd"]);
    assert_eq!(imported.warnings.len(), 5, "{:?}", imported.warnings);
    assert!(imported
        .warnings
        .iter()
        .all(|w| !w.contains(&*outside.path().to_string_lossy())));
}

#[tokio::test]
async fn limits_per_document_size_redirects_and_wall_clock() {
    let big = format!("{XSD}<!--{}-->", "x".repeat(10 * 1024 * 1024));
    let server = wsdl_server::spawn(
        Docs::default()
            .add("/calc.wsdl", calculator_importing(&["big.xsd"]))
            .add("/big.xsd", big),
    )
    .await;
    let six = (0..6).fold(server.url("/calc.wsdl"), |target, _| {
        server.url(&format!("/redirect?to={}", urlencode(&target)))
    });
    let s = setup();

    let sized = s
        .import(url_root(&server.url("/calc.wsdl")), Auth::None)
        .await
        .unwrap();
    let redirected = s.import(url_root(&six), Auth::None).await.unwrap_err();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket);
        }
    });
    let slow = WsdlService::new(s._dir.path()).with_limits(WalkLimits {
        wall: Duration::from_millis(200),
        ..WalkLimits::default()
    });
    let fetch = EngineFetch {
        engine: &s.engine,
        request_id: uuid::Uuid::new_v4().to_string(),
        location: RequestLocation::Scratch { id: "s".into() },
        auth: Auth::None,
    };
    let timed_out = slow
        .import(
            &fetch,
            SCOPE,
            url_root(&format!("http://{addr}/a.wsdl")),
            &|t: &str| t.to_string(),
        )
        .await
        .unwrap_err();

    assert!(
        sized.warnings[0].contains("larger than"),
        "{:?}",
        sized.warnings
    );
    assert!(
        redirected.message.contains("redirected too many times"),
        "{}",
        redirected.message
    );
    assert_eq!(timed_out.code, ErrorCode::Timeout);
}

fn urlencode(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes()).collect()
}

#[tokio::test]
async fn xml_with_a_dtd_is_refused() {
    let server = wsdl_server::spawn(
        Docs::default()
            .fixture("/xxe.wsdl", "bad-doctype-xxe.wsdl")
            .fixture("/laughs.wsdl", "bad-billion-laughs.wsdl"),
    )
    .await;
    let s = setup();

    for path in ["/xxe.wsdl", "/laughs.wsdl"] {
        let error = s
            .import(url_root(&server.url(path)), Auth::None)
            .await
            .unwrap_err();
        assert_eq!(error.message, "DTDs are not allowed in WSDL imports.");
    }
}

#[tokio::test]
async fn the_report_lists_masked_locations_and_skipped_imports() {
    let server = wsdl_server::spawn(
        Docs::default().add("/calc.wsdl", calculator_importing(&["missing.xsd"])),
    )
    .await;
    let s = setup();
    let typed = format!("http://u:pw@{}/calc.wsdl?key=s3cret", server.addr);

    let imported = s.import(url_root(&typed), Auth::None).await.unwrap();

    assert_eq!(
        imported.source,
        format!("http://{}/calc.wsdl?key=[REDACTED]", server.addr)
    );
    assert_eq!(
        imported.fetched,
        vec![format!("http://{}/calc.wsdl", server.addr)]
    );
    assert!(
        imported.warnings[0].starts_with("Skipped"),
        "{:?}",
        imported.warnings
    );
    assert!(imported
        .warnings
        .iter()
        .all(|w| !w.contains("s3cret") && !w.contains("pw@")));
}
