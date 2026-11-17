//! HTTP Digest authentication (RFC 7616, RFC 2617 fallback; plan 03b §6.1). Pure: parses
//! `WWW-Authenticate` challenges, picks the strongest supported Digest challenge and
//! computes the `Authorization` value. `follow` decides when to answer.

use std::fmt;
use std::fmt::Write as _;

use md5::{Digest as _, Md5};

use crate::error::AppError;

/// Every answer is the first use of a fresh challenge: no nonce cache between sends.
const NONCE_COUNT: &str = "00000001";
const CNONCE_BYTES: usize = 16;

/// The Digest username and password of a request; the password never reaches a header.
#[derive(Clone, PartialEq, Eq)]
pub struct DigestCredentials {
    pub username: String,
    pub password: String,
}

impl fmt::Debug for DigestCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DigestCredentials")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// Supported algorithms, weakest first (the order decides which challenge wins).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Algorithm {
    Md5,
    Md5Sess,
    Sha256,
    Sha256Sess,
}

impl Algorithm {
    fn parse(text: &str) -> Option<Self> {
        [
            ("MD5", Self::Md5),
            ("MD5-sess", Self::Md5Sess),
            ("SHA-256", Self::Sha256),
            ("SHA-256-sess", Self::Sha256Sess),
        ]
        .into_iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(text))
        .map(|(_, algorithm)| algorithm)
    }

    fn name(self) -> &'static str {
        match self {
            Self::Md5 => "MD5",
            Self::Md5Sess => "MD5-sess",
            Self::Sha256 => "SHA-256",
            Self::Sha256Sess => "SHA-256-sess",
        }
    }

    fn is_sess(self) -> bool {
        matches!(self, Self::Md5Sess | Self::Sha256Sess)
    }

    fn hash(self, text: &str) -> String {
        match self {
            Self::Md5 | Self::Md5Sess => hex(&Md5::digest(text.as_bytes())),
            Self::Sha256 | Self::Sha256Sess => {
                hex(aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, text.as_bytes()).as_ref())
            }
        }
    }
}

/// A Digest challenge this client can answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    pub realm: String,
    pub nonce: String,
    pub opaque: Option<String>,
    pub algorithm: Algorithm,
    /// The challenge named its algorithm: the answer repeats it (RFC 2617 omits it).
    pub algorithm_named: bool,
    /// `qop=auth` offered; otherwise the RFC 2069 form is used.
    pub qop_auth: bool,
}

/// What a `401` offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    /// No Digest challenge (for example only `Basic`): the password is never sent.
    NoDigest,
    /// Only Digest forms this client does not support (`auth-int`, `userhash`, …).
    Unsupported,
    Supported(Challenge),
}

/// A challenge as written: scheme and lower-cased parameter names.
#[derive(Debug, Default)]
struct RawChallenge {
    scheme: String,
    params: Vec<(String, String)>,
}

impl RawChallenge {
    fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

fn is_delimiter(c: char) -> bool {
    c.is_ascii_whitespace() || matches!(c, ',' | '=' | '"')
}

/// A quoted string (escapes undone) or a bare token, and the text after it.
fn read_value(text: &str) -> (String, &str) {
    let Some(quoted) = text.strip_prefix('"') else {
        let end = text
            .find(|c: char| c == ',' || c.is_ascii_whitespace())
            .unwrap_or(text.len());
        return (text[..end].to_string(), &text[end..]);
    };
    let mut value = String::new();
    let mut chars = quoted.char_indices();
    while let Some((index, c)) = chars.next() {
        match c {
            '\\' => value.extend(chars.next().map(|(_, escaped)| escaped)),
            '"' => return (value, &quoted[index + 1..]),
            _ => value.push(c),
        }
    }
    (value, "")
}

/// Every challenge in one `WWW-Authenticate` value (a value may hold several).
fn parse_challenges(header: &str) -> Vec<RawChallenge> {
    let mut challenges: Vec<RawChallenge> = Vec::new();
    let mut rest = header;
    loop {
        rest = rest.trim_start_matches(|c: char| c == ',' || c.is_ascii_whitespace());
        let Some(first) = rest.chars().next() else {
            return challenges;
        };
        let end = rest.find(is_delimiter).unwrap_or(rest.len());
        if end == 0 {
            // A stray `=` or quoted string: skip it.
            let (_, after) = read_value(rest);
            rest = if after.len() < rest.len() {
                after
            } else {
                &rest[first.len_utf8()..]
            };
            continue;
        }
        let token = &rest[..end];
        let after = rest[end..].trim_start_matches([' ', '\t']);
        match after.strip_prefix('=') {
            Some(value) => {
                let (value, next) = read_value(value.trim_start_matches([' ', '\t']));
                if let Some(last) = challenges.last_mut() {
                    last.params.push((token.to_ascii_lowercase(), value));
                }
                rest = next;
            }
            None => {
                challenges.push(RawChallenge {
                    scheme: token.to_string(),
                    ..RawChallenge::default()
                });
                rest = &rest[end..];
            }
        }
    }
}

fn supported(raw: &RawChallenge) -> Option<Challenge> {
    if raw
        .param("userhash")
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    {
        return None;
    }
    let algorithm = match raw.param("algorithm") {
        Some(name) => Algorithm::parse(name)?,
        None => Algorithm::Md5,
    };
    let qop_auth = match raw.param("qop") {
        Some(list) => list
            .split(',')
            .any(|qop| qop.trim().eq_ignore_ascii_case("auth"))
            .then_some(true)?,
        None => false,
    };
    if algorithm.is_sess() && !qop_auth {
        return None;
    }
    Some(Challenge {
        realm: raw.param("realm")?.to_string(),
        nonce: raw.param("nonce")?.to_string(),
        opaque: raw.param("opaque").map(str::to_string),
        algorithm,
        algorithm_named: raw.param("algorithm").is_some(),
        qop_auth,
    })
}

/// The strongest supported Digest challenge among every `WWW-Authenticate` value.
pub fn select<'a>(headers: impl IntoIterator<Item = &'a str>) -> Offer {
    let digest: Vec<RawChallenge> = headers
        .into_iter()
        .flat_map(parse_challenges)
        .filter(|raw| raw.scheme.eq_ignore_ascii_case("digest"))
        .collect();
    if digest.is_empty() {
        return Offer::NoDigest;
    }
    digest
        .iter()
        .filter_map(supported)
        .max_by_key(|challenge| challenge.algorithm)
        .map_or(Offer::Unsupported, Offer::Supported)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

fn quoted(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// A fresh client nonce: 16 random bytes, hex.
pub fn new_cnonce() -> Result<String, AppError> {
    use aws_lc_rs::rand::SecureRandom as _;
    let mut bytes = [0_u8; CNONCE_BYTES];
    aws_lc_rs::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| AppError::internal("Could not generate a random value."))?;
    Ok(hex(&bytes))
}

/// The `Authorization` value answering `challenge` for `method` on `uri` (the request
/// target: path and query).
pub fn authorization(
    challenge: &Challenge,
    credentials: &DigestCredentials,
    method: &str,
    uri: &str,
    cnonce: &str,
) -> String {
    let algorithm = challenge.algorithm;
    let nonce = &challenge.nonce;
    let secret = algorithm.hash(&format!(
        "{}:{}:{}",
        credentials.username, challenge.realm, credentials.password
    ));
    let ha1 = if algorithm.is_sess() {
        algorithm.hash(&format!("{secret}:{nonce}:{cnonce}"))
    } else {
        secret
    };
    let ha2 = algorithm.hash(&format!("{method}:{uri}"));
    let response = if challenge.qop_auth {
        algorithm.hash(&format!("{ha1}:{nonce}:{NONCE_COUNT}:{cnonce}:auth:{ha2}"))
    } else {
        algorithm.hash(&format!("{ha1}:{nonce}:{ha2}"))
    };
    let fields = [
        Some(format!("username={}", quoted(&credentials.username))),
        Some(format!("realm={}", quoted(&challenge.realm))),
        Some(format!("uri={}", quoted(uri))),
        challenge
            .algorithm_named
            .then(|| format!("algorithm={}", algorithm.name())),
        Some(format!("nonce={}", quoted(nonce))),
        challenge
            .qop_auth
            .then(|| format!("nc={NONCE_COUNT}, cnonce={}, qop=auth", quoted(cnonce))),
        Some(format!("response={}", quoted(&response))),
        challenge
            .opaque
            .as_ref()
            .map(|opaque| format!("opaque={}", quoted(opaque))),
    ];
    let joined: Vec<String> = fields.into_iter().flatten().collect();
    format!("Digest {}", joined.join(", "))
}

#[cfg(test)]
#[path = "digest_tests.rs"]
mod tests;
