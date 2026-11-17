use super::*;
use crate::engine::http::network::Network;
use crate::vars::vault::MemoryVault;

fn config() -> Arc<NetConfig> {
    let dir = tempfile::tempdir().unwrap();
    Network::load(dir.path(), Arc::new(MemoryVault::default()))
        .current()
        .unwrap()
}

fn key(jar: Option<&str>, verify_tls: bool, version: HttpVersion) -> ClientKey {
    ClientKey {
        jar: jar.map(|jar| effective_jar(jar, verify_tls)),
        verify_tls,
        http_version: version,
        identity: None,
        network_generation: 0,
    }
}

#[test]
fn scratch_requests_share_a_jar_and_collections_have_their_own() {
    let scratch = RequestLocation::Scratch { id: "a".into() };
    let collection = RequestLocation::Collection {
        collection_id: "c1".into(),
        path: "x.http.yaml".into(),
    };

    assert_eq!(jar_key(&scratch), "scratch");
    assert_eq!(jar_key(&collection), "collection:c1");
}

#[test]
fn clients_are_cached_per_key() {
    let (clients, config) = (Clients::default(), config());

    for key in [
        key(Some("a"), true, HttpVersion::Auto),
        key(Some("a"), true, HttpVersion::Auto),
        key(Some("a"), false, HttpVersion::Auto),
        key(Some("a"), true, HttpVersion::Http2),
        key(None, true, HttpVersion::Auto),
        ClientKey {
            identity: Some("id".into()),
            ..key(Some("a"), true, HttpVersion::Auto)
        },
    ] {
        clients.client(&key, &config, None).unwrap();
    }
    clients.clear("a");

    assert_eq!(recover(&clients.cache).entries.len(), 5);
    assert_eq!(recover(&clients.jars).len(), 2);
}

#[test]
fn a_new_network_generation_empties_the_cache() {
    let (clients, config) = (Clients::default(), config());
    clients
        .client(&key(Some("a"), true, HttpVersion::Auto), &config, None)
        .unwrap();
    let newer = ClientKey {
        network_generation: 1,
        ..key(Some("a"), true, HttpVersion::Auto)
    };

    clients.client(&newer, &config, None).unwrap();

    let cache = recover(&clients.cache);
    assert_eq!(cache.entries.len(), 1);
    assert!(cache.entries.contains_key(&newer));
}

#[test]
fn the_least_recently_used_client_is_dropped_beyond_the_cap() {
    let (clients, config) = (Clients::default(), config());
    let named = |i: usize| key(Some(&format!("j{i}")), true, HttpVersion::Auto);
    for i in 0..MAX_CLIENTS {
        clients.client(&named(i), &config, None).unwrap();
    }
    clients.client(&named(0), &config, None).unwrap();

    clients.client(&named(MAX_CLIENTS), &config, None).unwrap();

    let cache = recover(&clients.cache);
    assert_eq!(cache.entries.len(), MAX_CLIENTS);
    assert!(cache.entries.contains_key(&named(0)), "recently used");
    assert!(
        !cache.entries.contains_key(&named(1)),
        "least recently used"
    );
}

#[test]
fn a_slow_build_from_an_older_generation_does_not_replace_the_newer_cache() {
    let (clients, config) = (Clients::default(), config());
    let newer = ClientKey {
        network_generation: 1,
        ..key(Some("a"), true, HttpVersion::Auto)
    };
    clients.client(&newer, &config, None).unwrap();

    clients
        .client(&key(Some("a"), true, HttpVersion::Auto), &config, None)
        .unwrap();

    let cache = recover(&clients.cache);
    assert_eq!(cache.generation, 1);
    assert_eq!(cache.entries.len(), 1);
    assert!(cache.entries.contains_key(&newer));
}

/// A vault that counts reads of certificate values.
#[derive(Default)]
struct CountingVault {
    inner: MemoryVault,
    cert_reads: std::sync::atomic::AtomicUsize,
}

impl SecretStore for CountingVault {
    fn get(&self, id: &str) -> Result<Option<String>, AppError> {
        if id.starts_with("n/cert/") {
            self.cert_reads
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        self.inner.get(id)
    }
    fn has(&self, id: &str) -> Result<bool, AppError> {
        self.inner.has(id)
    }
    fn apply(&self, changes: &[crate::vars::vault::SecretChange]) -> Result<(), AppError> {
        self.inner.apply(changes)
    }
    fn delete_prefix(&self, prefix: &str) -> Result<(), AppError> {
        self.inner.delete_prefix(prefix)
    }
    fn status(&self) -> crate::vars::types::SecretStorageStatus {
        self.inner.status()
    }
    fn reset(&self) -> Result<(), AppError> {
        self.inner.reset()
    }
}

struct WithCertificate {
    _dir: tempfile::TempDir,
    vault: Arc<CountingVault>,
    network: Network,
}

fn with_certificate(host: &str) -> WithCertificate {
    let dir = tempfile::tempdir().unwrap();
    let signer = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec![host.to_string()])
        .unwrap()
        .self_signed(&signer)
        .unwrap();
    let bundle = dir.path().join("client.pem");
    std::fs::write(&bundle, format!("{}{}", cert.pem(), signer.serialize_pem())).unwrap();
    let vault = Arc::new(CountingVault::default());
    let network = Network::load(dir.path(), Arc::clone(&vault) as Arc<dyn SecretStore>);
    network.add_certificate(host, None, &bundle, None).unwrap();
    WithCertificate {
        _dir: dir,
        vault,
        network,
    }
}

fn selector(clients: &Arc<Clients>, setup: &WithCertificate) -> ClientSelector {
    ClientSelector::new(
        Arc::clone(clients),
        Arc::clone(&setup.vault) as Arc<dyn SecretStore>,
        setup.network.current().unwrap(),
        Some("scratch"),
        true,
        HttpVersion::Auto,
    )
}

#[tokio::test]
async fn a_cached_client_is_reused_without_reading_the_certificate_again() {
    let setup = with_certificate("client.example");
    let clients = Arc::new(Clients::default());
    let url = Url::parse("https://client.example/a").unwrap();
    let reads = || {
        setup
            .vault
            .cert_reads
            .load(std::sync::atomic::Ordering::SeqCst)
    };
    let before = reads();

    selector(&clients, &setup).for_url(&url).await.unwrap();
    selector(&clients, &setup).for_url(&url).await.unwrap();

    assert_eq!(reads() - before, 1);
}

#[tokio::test]
async fn a_missing_certificate_value_fails_the_hop_only_when_a_client_is_built() {
    let setup = with_certificate("client.example");
    let url = Url::parse("https://client.example/a").unwrap();
    let cached = Arc::new(Clients::default());
    selector(&cached, &setup).for_url(&url).await.unwrap();
    setup.vault.inner.delete_prefix("n/cert/").unwrap();

    let reused = selector(&cached, &setup).for_url(&url).await;
    let built = selector(&Arc::new(Clients::default()), &setup)
        .for_url(&url)
        .await;

    assert!(reused.is_ok());
    assert_eq!(
        built.unwrap_err().code,
        crate::error::ErrorCode::InvalidInput
    );
}

fn use_proxy(setup: &WithCertificate, url: &str, bypass: &[&str]) {
    let input = crate::engine::http::types::ProxyInput {
        mode: crate::engine::http::types::ProxyMode::Custom,
        url: url.to_string(),
        username: String::new(),
        password: crate::vars::types::SecretEdit::Keep,
        bypass: bypass.iter().map(|pattern| pattern.to_string()).collect(),
    };
    setup.network.write_proxy(&input).unwrap();
}

#[tokio::test]
async fn a_client_certificate_is_refused_through_an_https_proxy() {
    let setup = with_certificate("client.example");
    let clients = Arc::new(Clients::default());
    let url = Url::parse("https://client.example/a").unwrap();
    let elsewhere = Url::parse("https://other.example/a").unwrap();

    use_proxy(&setup, "https://proxy.corp:3128", &[]);
    let refused = selector(&clients, &setup).for_url(&url).await;
    let no_certificate = selector(&clients, &setup).for_url(&elsewhere).await;
    use_proxy(&setup, "https://proxy.corp:3128", &["*.example"]);
    let bypassed = selector(&clients, &setup).for_url(&url).await;
    use_proxy(&setup, "http://proxy.corp:3128", &[]);
    let plain_proxy = selector(&clients, &setup).for_url(&url).await;

    let error = refused.unwrap_err();
    assert_eq!(error.code, crate::error::ErrorCode::InvalidInput);
    assert!(error.message.contains("HTTPS proxy"), "{}", error.message);
    assert!(no_certificate.is_ok() && bypassed.is_ok() && plain_proxy.is_ok());
}
