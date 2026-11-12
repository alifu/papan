use super::*;

#[derive(serde::Deserialize)]
struct Vector {
    name: String,
    challenge: String,
    method: String,
    uri: String,
    username: String,
    password: String,
    nc: String,
    cnonce: String,
    expected: String,
}

fn vectors() -> Vec<Vector> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/digest/vectors.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn challenge_of(headers: &[&str]) -> Challenge {
    match select(headers.iter().copied()) {
        Offer::Supported(challenge) => challenge,
        other => panic!("not supported: {other:?}"),
    }
}

fn mufasa() -> DigestCredentials {
    DigestCredentials {
        username: "Mufasa".into(),
        password: "Circle of Life".into(),
    }
}

#[test]
fn the_rfc_vectors_produce_the_expected_authorization() {
    for vector in vectors() {
        let challenge = challenge_of(&[&vector.challenge]);
        let credentials = DigestCredentials {
            username: vector.username,
            password: vector.password,
        };
        assert_eq!(vector.nc, NONCE_COUNT, "{}", vector.name);

        let header = authorization(
            &challenge,
            &credentials,
            &vector.method,
            &vector.uri,
            &vector.cnonce,
        );

        assert_eq!(header, vector.expected, "{}", vector.name);
    }
}

#[test]
fn the_strongest_supported_challenge_wins() {
    let headers = [
        "Basic realm=\"b\", Digest realm=\"r\", nonce=\"n1\", algorithm=MD5, qop=\"auth\"",
        "Digest realm=\"r\", nonce=\"n3\", algorithm=SHA-512-256, qop=\"auth\"",
        "Digest realm=\"r\", nonce=\"n2\", algorithm=SHA-256, qop=\"auth\"",
    ];

    let challenge = challenge_of(&headers);

    assert_eq!(
        (challenge.algorithm, challenge.nonce.as_str()),
        (Algorithm::Sha256, "n2")
    );
}

#[test]
fn quoted_strings_keep_escapes_and_commas() {
    let header = r#"Digest realm="a \"quoted\", realm", nonce="n,1", opaque="x\\y", qop="auth""#;

    let challenge = challenge_of(&[header]);

    assert_eq!(challenge.realm, "a \"quoted\", realm");
    assert_eq!(challenge.nonce, "n,1");
    assert_eq!(challenge.opaque.as_deref(), Some("x\\y"));
    assert!(challenge.qop_auth);
    let answer = authorization(&challenge, &mufasa(), "GET", "/", "c");
    assert!(
        answer.contains(r#"realm="a \"quoted\", realm""#),
        "{answer}"
    );
}

#[test]
fn unsupported_forms_are_not_answered() {
    for header in [
        "Digest realm=\"r\", nonce=\"n\", qop=\"auth-int\"",
        "Digest realm=\"r\", nonce=\"n\", qop=\"auth\", userhash=true",
        "Digest realm=\"r\", nonce=\"n\", algorithm=SHA-512-256",
        "Digest realm=\"r\", nonce=\"n\", algorithm=MD5-sess",
        "Digest realm=\"r\"",
    ] {
        assert_eq!(select([header]), Offer::Unsupported, "{header}");
    }
}

#[test]
fn a_basic_or_absent_challenge_is_not_digest() {
    assert_eq!(select(["Basic realm=\"x\""]), Offer::NoDigest);
    assert_eq!(select(["Bearer abc==, Negotiate"]), Offer::NoDigest);
    assert_eq!(select(std::iter::empty()), Offer::NoDigest);
    assert_eq!(select(["\"=,=\" ,"]), Offer::NoDigest);
}

#[test]
fn the_rfc_2069_form_has_no_qop_fields() {
    let challenge = challenge_of(&["Digest realm=\"r\", nonce=\"n\""]);

    let answer = authorization(&challenge, &mufasa(), "GET", "/a?b=1", "c");

    assert!(!challenge.qop_auth);
    assert!(
        !answer.contains("qop") && !answer.contains("cnonce"),
        "{answer}"
    );
    assert!(answer.contains("uri=\"/a?b=1\""), "{answer}");
}

#[test]
fn sess_algorithms_mix_the_client_nonce_into_the_secret() {
    let challenge =
        challenge_of(&["Digest realm=\"r\", nonce=\"n\", algorithm=MD5-sess, qop=auth"]);

    let first = authorization(&challenge, &mufasa(), "GET", "/", "c1");
    let second = authorization(&challenge, &mufasa(), "GET", "/", "c2");

    assert!(first.contains("algorithm=MD5-sess"));
    assert_ne!(
        first.split("response=").nth(1),
        second.split("response=").nth(1)
    );
}

#[test]
fn client_nonces_are_sixteen_random_bytes() {
    let (a, b) = (new_cnonce().unwrap(), new_cnonce().unwrap());

    assert_eq!(a.len(), CNONCE_BYTES * 2);
    assert_ne!(a, b);
}

#[test]
fn debug_output_hides_the_password() {
    assert!(!format!("{:?}", mufasa()).contains("Circle"));
}
