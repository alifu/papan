use super::*;
use crate::error::ErrorCode;
use crate::vars::vault::MemoryVault;

const PASSWORD: &str = "pr0xy-s3cret";

fn custom(password: SecretEdit) -> ProxyInput {
    ProxyInput {
        mode: ProxyMode::Custom,
        url: "http://proxy.corp:3128".to_string(),
        username: "alifu".to_string(),
        password,
        bypass: vec!["localhost".to_string()],
    }
}

fn network(dir: &Path) -> (Network, Arc<MemoryVault>) {
    let vault = Arc::new(MemoryVault::default());
    (Network::load(dir, vault.clone()), vault)
}

#[test]
fn the_proxy_password_is_kept_in_the_vault_and_never_in_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let (network, vault) = network(dir.path());

    let shown = network
        .write_proxy(&custom(SecretEdit::Set {
            value: PASSWORD.to_string(),
        }))
        .unwrap();

    let file = std::fs::read_to_string(dir.path().join(NETWORK_FILE)).unwrap();
    assert!(!file.contains(PASSWORD), "{file}");
    assert!(file.contains("proxy.corp"));
    assert_eq!(
        vault.get(PROXY_PASSWORD_ID).unwrap().as_deref(),
        Some(PASSWORD)
    );
    assert!(shown.proxy.has_password);
    assert!(!serde_json::to_string(&shown).unwrap().contains(PASSWORD));
    assert_eq!(network.current().unwrap().password(), Some(PASSWORD));
}

#[test]
fn keep_and_clear_follow_secret_edit() {
    let dir = tempfile::tempdir().unwrap();
    let (network, vault) = network(dir.path());
    let set = SecretEdit::Set {
        value: PASSWORD.to_string(),
    };
    network.write_proxy(&custom(set)).unwrap();

    let kept = network.write_proxy(&custom(SecretEdit::Keep)).unwrap();
    let cleared = network.write_proxy(&custom(SecretEdit::Clear)).unwrap();

    assert!(kept.proxy.has_password);
    assert!(!cleared.proxy.has_password);
    assert!(vault.get(PROXY_PASSWORD_ID).unwrap().is_none());
}

#[test]
fn an_empty_password_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (network, _) = network(dir.path());

    let error = network
        .write_proxy(&custom(SecretEdit::Set {
            value: String::new(),
        }))
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
}

#[test]
fn every_write_starts_a_new_generation() {
    let dir = tempfile::tempdir().unwrap();
    let (network, _) = network(dir.path());
    let before = network.current().unwrap().generation;

    network.write_proxy(&custom(SecretEdit::Keep)).unwrap();

    assert_ne!(network.current().unwrap().generation, before);
}

#[test]
fn settings_survive_a_reload_and_a_malformed_file_gives_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let (network, vault) = network(dir.path());
    network.write_proxy(&custom(SecretEdit::Keep)).unwrap();

    let reloaded = Network::load(dir.path(), vault.clone()).settings().unwrap();
    std::fs::write(
        dir.path().join(NETWORK_FILE),
        b"{\"version\":1,\"proxy\":7}",
    )
    .unwrap();
    let broken = Network::load(dir.path(), vault).settings().unwrap();

    assert_eq!(reloaded.proxy.url, "http://proxy.corp:3128");
    assert_eq!(broken.proxy, ProxySettings::default());
}

#[test]
fn removing_an_unknown_certificate_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let (network, _) = network(dir.path());

    let error = network
        .remove_certificate("0b0e7a52-6a55-4c3e-9d0e-2f1c8a9b7d11")
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::NotFound);
}

fn at(url: &str, password: SecretEdit) -> ProxyInput {
    ProxyInput {
        url: url.to_string(),
        ..custom(password)
    }
}

#[test]
fn keeping_the_password_for_another_proxy_host_or_port_drops_it() {
    let dir = tempfile::tempdir().unwrap();
    let (network, vault) = network(dir.path());
    let set = SecretEdit::Set {
        value: PASSWORD.to_string(),
    };
    network.write_proxy(&custom(set)).unwrap();

    let same = network
        .write_proxy(&at("http://PROXY.corp:3128/", SecretEdit::Keep))
        .unwrap();
    let moved = network
        .write_proxy(&at("http://proxy.corp:8080", SecretEdit::Keep))
        .unwrap();

    assert!(same.proxy.has_password);
    assert!(!moved.proxy.has_password);
    assert!(vault.get(PROXY_PASSWORD_ID).unwrap().is_none());
    assert_eq!(network.current().unwrap().password(), None);
}

#[test]
fn an_edited_network_file_never_sends_the_password_to_another_host() {
    let dir = tempfile::tempdir().unwrap();
    let (network, vault) = network(dir.path());
    let set = SecretEdit::Set {
        value: PASSWORD.to_string(),
    };
    network.write_proxy(&custom(set)).unwrap();
    let path = dir.path().join(NETWORK_FILE);
    let edited = std::fs::read_to_string(&path)
        .unwrap()
        .replace("proxy.corp", "evil.example");
    std::fs::write(&path, edited).unwrap();

    let reloaded = Network::load(dir.path(), vault);

    assert_eq!(reloaded.current().unwrap().password(), None);
    assert!(!reloaded.settings().unwrap().proxy.has_password);
}

#[test]
fn the_proxy_mode_is_read_without_opening_the_vault() {
    let dir = tempfile::tempdir().unwrap();
    let (network, _) = network(dir.path());
    network.write_proxy(&custom(SecretEdit::Keep)).unwrap();
    let damaged = Arc::new(MemoryVault::new(
        crate::vars::types::SecretStorageMode::Damaged,
    ));

    let reloaded = Network::load(dir.path(), damaged);

    assert_eq!(reloaded.proxy_mode(), ProxyMode::Custom);
}
