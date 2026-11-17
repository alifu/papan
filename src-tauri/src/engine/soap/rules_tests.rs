use super::*;

fn url(text: &str) -> Url {
    Url::parse(text).unwrap()
}

#[test]
fn private_literals_are_recognised_and_hostnames_are_not_resolved() {
    let private = [
        "http://localhost/a",
        "http://api.localhost./a",
        "http://127.0.0.1/a",
        "http://10.1.2.3/a",
        "http://172.16.0.1/a",
        "http://192.168.1.1/a",
        "http://169.254.169.254/latest",
        "http://0.0.0.0/a",
        "http://[::1]/a",
        "http://[::]/a",
        "http://[fd00::1]/a",
        "http://[fe80::1]/a",
        "http://[::ffff:10.0.0.1]/a",
    ];
    let public = [
        "http://example.test/a",
        "http://8.8.8.8/a",
        "http://[2001:db8::1]/a",
        "http://intranet.corp/a",
    ];

    for text in private {
        assert!(is_private_literal(&url(text)), "{text}");
    }
    for text in public {
        assert!(!is_private_literal(&url(text)), "{text}");
    }
}

#[test]
fn only_http_and_https_are_followed() {
    let root = url("http://example.test/a.wsdl");

    for refused in [
        "file:///etc/passwd",
        "ftp://example.test/a.xsd",
        "data:text/xml,<a/>",
    ] {
        let error = check_url(&url(refused), Some(&root)).unwrap_err();
        assert!(error.message.contains("only http and https"), "{refused}");
    }
    assert!(check_url(&url("https://other.test/b.xsd"), Some(&root)).is_ok());
}

#[test]
fn a_public_root_cannot_import_a_private_literal_but_a_private_root_can() {
    let public_root = url("http://example.test/a.wsdl");
    let private_root = url("http://127.0.0.1:8080/a.wsdl");
    let lan = url("http://192.168.0.10/types.xsd");

    assert!(check_url(&lan, Some(&public_root)).is_err());
    assert!(check_url(&lan, None).is_err());
    assert!(check_url(&lan, Some(&private_root)).is_ok());
    assert!(check_url(&private_root, Some(&private_root)).is_ok());
}

#[test]
fn imports_resolve_against_the_importing_document() {
    let base = url("http://example.test/svc/main.wsdl?wsdl");

    assert_eq!(
        join_url(&base, " types.xsd ", Some(&base))
            .unwrap()
            .as_str(),
        "http://example.test/svc/types.xsd"
    );
    assert_eq!(
        join_url(&base, "../common/c.xsd", Some(&base))
            .unwrap()
            .as_str(),
        "http://example.test/common/c.xsd"
    );
    assert!(join_url(&base, "file:///etc/hosts", Some(&base)).is_err());
    assert!(join_url(&base, "http://[::1", Some(&base)).is_err());
}

#[test]
fn display_url_drops_credentials() {
    assert_eq!(
        display_url(&url("http://user:secret@example.test/a?wsdl")),
        "http://example.test/a?wsdl"
    );
}

#[test]
fn file_imports_stay_in_the_picked_folder_with_known_extensions() {
    let root = Path::new("/w");
    let doc = Path::new("/w/sub/main.wsdl");

    assert_eq!(
        join_file(doc, "types.xsd", root).unwrap(),
        Path::new("/w/sub/types.xsd")
    );
    assert_eq!(
        join_file(doc, "./../c.XML", root).unwrap(),
        Path::new("/w/c.XML")
    );
    for escape in [
        "../../etc/x.xsd",
        "/etc/x.xsd",
        "..\\x.xsd",
        "../../../../x.wsdl",
    ] {
        let error = join_file(doc, escape, root).unwrap_err();
        assert!(
            error.message.contains("outside"),
            "{escape}: {}",
            error.message
        );
    }
    for wrong in ["passwd", "key.pem", "x.xsd.txt"] {
        let error = join_file(doc, wrong, root).unwrap_err();
        assert!(error.message.contains(".wsdl, .xsd and .xml"), "{wrong}");
    }
}

#[test]
fn absolute_urls_are_told_from_paths() {
    assert!(is_absolute_url("http://x/a.xsd"));
    assert!(is_absolute_url("file:///etc/hosts"));
    assert!(!is_absolute_url("types.xsd"));
    assert!(!is_absolute_url("../a/b.xsd"));
    assert_eq!(
        display_path(Path::new("/w/a/b.xsd"), Path::new("/w")),
        "a/b.xsd"
    );
}

#[tokio::test]
async fn read_confined_refuses_symlinks_out_of_the_folder_and_large_files() {
    let outside = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.xsd"), "<x/>").unwrap();
    std::fs::write(root.path().join("ok.xsd"), "<ok/>").unwrap();
    std::fs::create_dir(root.path().join("dir.xsd")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        outside.path().join("secret.xsd"),
        root.path().join("link.xsd"),
    )
    .unwrap();

    let ok = read_confined(&root.path().join("ok.xsd"), root.path(), 100).await;
    let big = read_confined(&root.path().join("ok.xsd"), root.path(), 2).await;
    let dir = read_confined(&root.path().join("dir.xsd"), root.path(), 100).await;
    let missing = read_confined(&root.path().join("none.xsd"), root.path(), 100).await;

    assert_eq!(ok.unwrap(), b"<ok/>");
    assert!(big.unwrap_err().message.contains("larger than"));
    assert!(dir.unwrap_err().message.contains("not a file"));
    assert!(missing
        .unwrap_err()
        .message
        .contains("Could not read none.xsd"));
    #[cfg(unix)]
    {
        let link = read_confined(&root.path().join("link.xsd"), root.path(), 100).await;
        assert!(link.unwrap_err().message.contains("outside"));
    }
}

#[test]
fn embedded_ipv4_cgnat_this_network_and_site_local_are_private() {
    let private = [
        "http://[::127.0.0.1]/a",
        "http://[64:ff9b::a9fe:a9fe]/latest",
        "http://[2002:7f00:1::]/a",
        "http://[2002:c0a8:101::1]/a",
        "http://100.64.0.1/a",
        "http://100.127.255.254/a",
        "http://0.0.0.1/a",
        "http://[fec0::1]/a",
        "http://[::ffff:127.0.0.1]/a",
    ];
    let public = [
        "http://100.128.0.1/a",
        "http://100.63.255.255/a",
        "http://[64:ff9b::808:808]/a",
        "http://[2002:808:808::]/a",
        "http://[::808:808]/a",
    ];
    let root = url("http://example.test/a.wsdl");

    for text in private {
        assert!(is_private_literal(&url(text)), "{text}");
        assert!(check_url(&url(text), Some(&root)).is_err(), "{text}");
    }
    for text in public {
        assert!(!is_private_literal(&url(text)), "{text}");
    }
}
