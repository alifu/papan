//! cURL import and export against the H2 fixtures (plan 03b §10).

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use papan_lib::error::AppError;
use papan_lib::interop::curl::{export, export_masked, import, MAX_INPUT_BYTES};
use papan_lib::store::types::{
    Auth, Body, ClientAuth, KeyValue, OAuthGrant, RequestContent, RequestLocation,
};
use papan_lib::store::Store;
use papan_lib::vars::types::{SecretEdit, SecretStorageStatus, VarScope, VariableInput};
use papan_lib::vars::vault::{MemoryVault, SecretChange, SecretStore};
use papan_lib::vars::VarsService;
use serde::Deserialize;

const SECRET: &str = "tok-SECRET-9f2";
/// Export cases whose command imports as a different but equivalent request.
const LOSSY_EXPORTS: [&str; 4] = [
    "form-body",      // --data-urlencode imports as an encoded text body
    "bearer-auth",    // the Authorization header imports as a header
    "api-key-header", // likewise
    "api-key-query",  // the key imports as part of the URL
];

#[derive(Deserialize)]
struct ImportCase {
    name: String,
    command: String,
    content: Option<RequestContent>,
    warnings: Vec<String>,
}

#[derive(Deserialize)]
struct ExportCase {
    name: String,
    content: RequestContent,
    expected: String,
}

fn fixture<T: for<'de> Deserialize<'de>>(file: &str) -> Vec<T> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/curl")
        .join(file);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn every_import_case_gives_its_content_and_warnings() {
    let cases: Vec<ImportCase> = fixture("import-cases.json");
    assert_eq!(cases.len(), 38);

    for case in cases {
        let result = import(&case.command);

        match case.content {
            None => assert!(result.is_err(), "{}: should be refused", case.name),
            Some(content) => {
                let imported = result.unwrap_or_else(|error| panic!("{}: {error}", case.name));
                assert_eq!(imported.content, content, "{}", case.name);
                assert_eq!(imported.warnings, case.warnings, "{}", case.name);
            }
        }
    }
}

#[test]
fn every_export_case_gives_its_command() {
    let cases: Vec<ExportCase> = fixture("export-cases.json");
    assert_eq!(cases.len(), 18);

    for case in cases {
        assert_eq!(
            export(&case.content).unwrap(),
            case.expected,
            "{}",
            case.name
        );
    }
}

#[test]
fn importing_an_export_gives_the_content_back() {
    let cases: Vec<ExportCase> = fixture("export-cases.json");
    let names: Vec<&str> = cases.iter().map(|case| case.name.as_str()).collect();
    assert!(LOSSY_EXPORTS.iter().all(|name| names.contains(name)));

    for case in cases
        .iter()
        .filter(|case| !LOSSY_EXPORTS.contains(&case.name.as_str()))
    {
        let imported = import(&export(&case.content).unwrap()).unwrap();

        assert_eq!(imported.content, case.content, "{}", case.name);
        let only_tls = |warning: &String| warning.starts_with("-k: ");
        assert!(imported.warnings.iter().all(only_tls), "{}", case.name);
    }
}

#[test]
fn awkward_text_survives_a_round_trip() {
    let awkward = "it's \"q\" \\ $HOME `x` $(id)\n\ttab\ré{{v}}";
    let mut content = RequestContent::new("PATCH", "https://example.com/a b?x=[1]");
    content.headers = vec![
        KeyValue {
            name: "X-Odd".into(),
            value: awkward.into(),
            enabled: true,
        },
        KeyValue {
            name: "content-type".into(),
            value: "text/plain".into(),
            enabled: true,
        },
    ];
    content.body = Body::Text {
        text: awkward.into(),
    };
    content.settings.follow_redirects = false;

    let imported = import(&export(&content).unwrap()).unwrap();

    assert_eq!(imported.content, content);
}

#[test]
fn data_without_a_content_type_imports_with_the_form_type_curl_sends() {
    let given = import("curl -H 'content-type: text/csv' -d a,b https://e.x/").unwrap();
    let implied = import("curl --data-urlencode 'a=b c' https://e.x/").unwrap();
    let json = import("curl --json '{}' https://e.x/").unwrap();

    assert_eq!(given.content.headers.len(), 1);
    let form = KeyValue {
        name: "Content-Type".into(),
        value: "application/x-www-form-urlencoded".into(),
        enabled: true,
    };
    assert_eq!(implied.content.headers, [form]);
    assert_eq!(
        implied.content.body,
        Body::Text {
            text: "a=b%20c".into()
        }
    );
    assert!(json.content.headers.is_empty());
}

#[test]
fn import_reads_no_file_and_expands_nothing() {
    let command = "curl -d @/definitely/not/here.bin \"$HOME\"/x";

    let imported = import(command).unwrap();

    assert_eq!(
        imported.content.body,
        Body::Binary {
            file: Some("/definitely/not/here.bin".into())
        }
    );
    assert_eq!(imported.content.url, "$HOME/x");
}

#[test]
fn import_refuses_long_unquoted_and_url_less_text() {
    let long = format!("curl https://e.x/{}", "a".repeat(MAX_INPUT_BYTES));

    for text in [long.as_str(), "curl 'https://e.x", "curl -L", "", "  "] {
        assert!(import(text).is_err(), "{:.40}", text);
    }
}

/// Counts vault reads, so a test can prove export never makes one.
#[derive(Default)]
struct CountingVault {
    inner: MemoryVault,
    reads: AtomicUsize,
}

impl SecretStore for CountingVault {
    fn get(&self, id: &str) -> Result<Option<String>, AppError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.get(id)
    }
    fn has(&self, id: &str) -> Result<bool, AppError> {
        self.inner.has(id)
    }
    fn apply(&self, changes: &[SecretChange]) -> Result<(), AppError> {
        self.inner.apply(changes)
    }
    fn delete_prefix(&self, prefix: &str) -> Result<(), AppError> {
        self.inner.delete_prefix(prefix)
    }
    fn status(&self) -> SecretStorageStatus {
        self.inner.status()
    }
    fn reset(&self) -> Result<(), AppError> {
        self.inner.reset()
    }
}

fn row(name: &str, value: &str, secret: bool) -> VariableInput {
    VariableInput {
        name: name.into(),
        previous_name: None,
        value: if secret { String::new() } else { value.into() },
        current: None,
        enabled: true,
        secret,
        secret_edit: if secret {
            SecretEdit::Set {
                value: value.into(),
            }
        } else {
            SecretEdit::Keep
        },
    }
}

struct Exporter {
    _dir: tempfile::TempDir,
    store: Store,
    vars: VarsService,
    vault: Arc<CountingVault>,
}

fn exporter() -> Exporter {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(&dir.path().join("data")).unwrap();
    let vault = Arc::new(CountingVault::default());
    let vars = VarsService::new(vault.clone());
    let rows = [
        row("host", "api.example.com", false),
        row("auth", "Bearer {{api_token}}", false),
        row("api_token", SECRET, true),
    ];
    vars.write_variables(&mut store, &VarScope::Global, &rows)
        .unwrap();
    Exporter {
        _dir: dir,
        store,
        vars,
        vault,
    }
}

fn scratch() -> RequestLocation {
    RequestLocation::Scratch { id: "s1".into() }
}

#[test]
fn export_leaves_secrets_as_typed_and_never_reads_the_vault() {
    let fixture = exporter();
    let reads_before = fixture.vault.reads.load(Ordering::SeqCst);
    let mut content = RequestContent::new("GET", "https://{{host}}/items?t={{api_token}}");
    content.headers = vec![KeyValue {
        name: "X-Auth".into(),
        value: "{{auth}}".into(),
        enabled: true,
    }];
    content.auth = Auth::Basic {
        username: "me".into(),
        password: "{{api_token}}".into(),
    };

    let command = export_masked(&fixture.vars, &fixture.store, &scratch(), &content).unwrap();

    assert!(command.contains("'https://api.example.com/items?t={{api_token}}'"));
    assert!(command.contains("-H 'X-Auth: Bearer {{api_token}}'"));
    assert!(command.contains("-u 'me:{{api_token}}'"));
    assert!(!command.contains(SECRET));
    assert!(!command.contains("•"));
    assert_eq!(fixture.vault.reads.load(Ordering::SeqCst), reads_before);
}

#[test]
fn export_of_oauth_has_a_token_placeholder_and_no_client_secret() {
    let fixture = exporter();
    let mut content = RequestContent::new("GET", "https://{{host}}/me");
    content.auth = Auth::Oauth2 {
        grant: OAuthGrant::ClientCredentials,
        auth_url: String::new(),
        token_url: "https://idp.example.com/token".into(),
        client_id: "papan".into(),
        client_secret: "literal-client-secret".into(),
        scope: "read".into(),
        client_auth: ClientAuth::Basic,
        redirect_port: 0,
    };

    let command = export_masked(&fixture.vars, &fixture.store, &scratch(), &content).unwrap();

    assert!(command.contains("-H 'Authorization: Bearer <access token>'"));
    assert!(!command.contains("literal-client-secret"));
    assert!(!command.contains("idp.example.com"));
}

#[test]
fn export_refuses_a_body_from_a_newer_papan() {
    let fixture = exporter();
    let mut content = RequestContent::new("POST", "https://e.x/");
    content.body = Body::Unsupported;

    let result = export_masked(&fixture.vars, &fixture.store, &scratch(), &content);

    assert!(result.is_err());
}
