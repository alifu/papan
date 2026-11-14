//! WSDL import end to end over the real HTTP engine (plan 06 §11, `soap_import`).

mod support;

use std::path::Path;

use papan_lib::engine::http::HttpEngine;
use papan_lib::engine::soap::fetch::EngineFetch;
use papan_lib::engine::soap::service::{ImportRoot, WsdlService};
use papan_lib::engine::soap::EnvelopeOptions;
use papan_lib::error::ErrorCode;
use papan_lib::store::types::{Auth, RequestLocation, WsdlRef};
use support::wsdl_server::{self, Docs, PASSWORD, USER};
use url::Url;

const SCOPE: &str = "scratch:s";

fn location() -> RequestLocation {
    RequestLocation::Scratch { id: "s".into() }
}

fn basic() -> Auth {
    Auth::Basic {
        username: USER.into(),
        password: PASSWORD.into(),
    }
}

fn fixture_text(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/wsdl")
        .join(name);
    std::fs::read_to_string(path).unwrap()
}

fn fetcher<'a>(engine: &'a HttpEngine, auth: Auth) -> EngineFetch<'a> {
    EngineFetch {
        engine,
        request_id: uuid::Uuid::new_v4().to_string(),
        location: location(),
        auth,
    }
}

fn url_root(url: &str) -> ImportRoot {
    ImportRoot::Url {
        typed: url.into(),
        url: Url::parse(url).unwrap(),
    }
}

fn same(text: &str) -> String {
    text.to_string()
}

struct Setup {
    dir: tempfile::TempDir,
    engine: HttpEngine,
    service: WsdlService,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let engine = HttpEngine::new(&dir.path().join("cache"), dir.path(), "9.9.9").unwrap();
    let service = WsdlService::new(dir.path());
    Setup {
        dir,
        engine,
        service,
    }
}

#[tokio::test]
async fn imports_by_url_with_basic_auth_on_the_root_and_none_cross_origin() {
    let other =
        wsdl_server::spawn(Docs::default().fixture("/common.xsd", "split/common.xsd")).await;
    let types = fixture_text("split/types.xsd").replace(
        r#"schemaLocation="common.xsd""#,
        &format!(r#"schemaLocation="{}""#, other.url("/common.xsd")),
    );
    let root = wsdl_server::spawn(
        Docs::default()
            .fixture("/secure/main.wsdl", "split/main.wsdl")
            .add("/secure/types.xsd", types),
    )
    .await;
    let s = setup();

    let imported = s
        .service
        .import(
            &fetcher(&s.engine, basic()),
            SCOPE,
            url_root(&root.url("/secure/main.wsdl")),
            &same,
        )
        .await
        .unwrap();

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.fetched.len(), 3);
    assert_eq!(
        imported.services[0].ports[0].operations[0].name,
        "GetCustomer"
    );
    assert!(root.hits().iter().all(|hit| hit
        .authorization
        .as_deref()
        .is_some_and(|a| a.starts_with("Basic "))));
    assert_eq!(other.hits().len(), 1);
    assert_eq!(other.hits()[0].authorization, None);
    assert_eq!(other.hits()[0].cookie, None);
    let template = s
        .service
        .envelope(
            &imported.id,
            "CustomerService",
            "CustomerPort",
            "GetCustomer",
            EnvelopeOptions::default(),
        )
        .unwrap();
    assert!(template.envelope.contains("<ns1:GetCustomer>"));
}

#[tokio::test]
async fn without_credentials_a_secure_root_fails_with_the_status() {
    let root =
        wsdl_server::spawn(Docs::default().fixture("/secure/calc.wsdl", "calculator.wsdl")).await;
    let s = setup();

    let error = s
        .service
        .import(
            &fetcher(&s.engine, Auth::None),
            SCOPE,
            url_root(&root.url("/secure/calc.wsdl")),
            &same,
        )
        .await
        .unwrap_err();

    assert!(error.message.contains("401"), "{}", error.message);
}

#[tokio::test]
async fn a_redirect_to_file_is_refused() {
    let root = wsdl_server::spawn(Docs::default()).await;
    let s = setup();
    let url = root.url("/redirect?to=file:///etc/hosts");

    let error = s
        .service
        .import(
            &fetcher(&s.engine, Auth::None),
            SCOPE,
            url_root(&url),
            &same,
        )
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert_eq!(root.hits().len(), 1);
}

#[tokio::test]
async fn imports_a_picked_file_with_its_sibling_schemas() {
    let s = setup();
    let folder = tempfile::tempdir().unwrap();
    for name in ["main.wsdl", "types.xsd", "common.xsd"] {
        std::fs::write(
            folder.path().join(name),
            fixture_text(&format!("split/{name}")),
        )
        .unwrap();
    }
    let path = std::fs::canonicalize(folder.path().join("main.wsdl")).unwrap();

    let imported = s
        .service
        .import(
            &fetcher(&s.engine, Auth::None),
            SCOPE,
            ImportRoot::File { path },
            &same,
        )
        .await
        .unwrap();

    assert_eq!(imported.source, "main.wsdl");
    assert_eq!(
        imported.fetched,
        vec!["main.wsdl", "types.xsd", "common.xsd"]
    );
}

#[tokio::test]
async fn the_cache_is_reused_by_read_cached_wsdl_without_network() {
    let root = wsdl_server::spawn(Docs::default().fixture("/calc.wsdl", "calculator.wsdl")).await;
    let s = setup();
    let url = root.url("/calc.wsdl");

    let imported = s
        .service
        .import(
            &fetcher(&s.engine, Auth::None),
            SCOPE,
            url_root(&url),
            &same,
        )
        .await
        .unwrap();
    let hits = root.hits().len();
    let reference = WsdlRef {
        url: url.clone(),
        service: "Calculator".into(),
        port: "CalculatorSoap12".into(),
        operation: "Add".into(),
        ..WsdlRef::default()
    };
    let reopened = WsdlService::new(s.dir.path());

    assert_eq!(reopened.cached(SCOPE, &reference).unwrap(), Some(imported));
    assert_eq!(root.hits().len(), hits);
    assert!(s.dir.path().join("wsdl").is_dir());
}

#[tokio::test]
async fn cancelling_the_import_id_stops_it() {
    // A server that never answers: the import waits until cancelled.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket);
        }
    });
    let s = setup();
    let fetch = fetcher(&s.engine, Auth::None);
    let id = fetch.request_id.clone();
    let url = format!("http://{addr}/calc.wsdl");

    let import = s.service.import(&fetch, SCOPE, url_root(&url), &same);
    let cancel = async {
        while s.engine.in_flight() == 0 {
            tokio::task::yield_now().await;
        }
        s.engine.cancel(&id).unwrap();
    };
    let (result, ()) = tokio::join!(import, cancel);

    assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
}
