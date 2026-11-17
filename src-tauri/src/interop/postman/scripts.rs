//! Script mapping and the unsupported-API report (plan 09 §5). Scripts are imported
//! verbatim; this light lexer blanks comments, strings and template literals (keeping
//! line breaks) and lists the Postman APIs our sandbox (`script/prelude.js`) lacks.

use std::collections::BTreeSet;

/// Members of `pm` that `prelude.js` defines (kept equal by a drift test).
pub const PM_MEMBERS: [&str; 9] = [
    "collectionVariables",
    "environment",
    "expect",
    "globals",
    "request",
    "response",
    "sendRequest",
    "test",
    "variables",
];
const SCOPE_MEMBERS: [&str; 4] = ["collectionVariables", "environment", "globals", "variables"];
const SCOPE_METHODS: [&str; 5] = ["get", "has", "set", "toObject", "unset"];
const REQUEST_MEMBERS: [&str; 4] = ["body", "headers", "method", "url"];
const HEADER_METHODS: [&str; 7] = ["add", "each", "get", "has", "remove", "toObject", "upsert"];
const RESPONSE_MEMBERS: [&str; 7] = [
    "code",
    "headers",
    "json",
    "responseTime",
    "status",
    "text",
    "to",
];
const RESPONSE_TO_MEMBERS: [&str; 3] = ["be", "have", "not"];
const RESPONSE_UNSUPPORTED: [&str; 2] = ["jsonBody", "jsonSchema"];
const LEGACY_GLOBALS: [&str; 3] = ["responseBody", "responseCode", "responseHeaders"];
const LIBRARIES: [&str; 7] = [
    "CryptoJS", "_", "ajv", "cheerio", "moment", "tv4", "xml2Json",
];
const ASYNC_NAMES: [&str; 3] = ["async", "setInterval", "setTimeout"];

/// One use of an API the sandbox lacks (or, for `pm.sendRequest`, one worth knowing).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScriptFinding {
    /// 1-based line in the script.
    pub line: u32,
    pub api: String,
    /// `pm.sendRequest`: supported but it sends data out; reported as information.
    pub supported: bool,
}

/// Joins Postman `exec` lines with `\n` and several scripts of one kind with a blank line.
pub fn join_exec(scripts: &[Vec<String>]) -> String {
    scripts
        .iter()
        .map(|lines| lines.join("\n"))
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Unsupported API uses in `script`, sorted by line, one per API and line.
pub fn scan(script: &str) -> Vec<ScriptFinding> {
    let code = blank_literals(script);
    let found: BTreeSet<ScriptFinding> = chains(&code)
        .into_iter()
        .filter_map(|chain| classify(&chain))
        .collect();
    found.into_iter().collect()
}

/// A member chain like `pm.response.to.have` with what follows it.
#[derive(Debug)]
struct Chain {
    line: u32,
    parts: Vec<String>,
    next: Option<char>,
}

fn classify(chain: &Chain) -> Option<ScriptFinding> {
    let parts: Vec<&str> = chain.parts.iter().map(String::as_str).collect();
    let finding = |api: String, supported: bool| ScriptFinding {
        line: chain.line,
        api,
        supported,
    };
    match parts.as_slice() {
        ["pm"] => None,
        ["pm", rest @ ..] => classify_pm(rest).map(|(api, ok)| finding(api, ok)),
        ["postman", member, ..] => Some(finding(format!("postman.{member}"), false)),
        ["tests", ..] if chain.next == Some('[') => Some(finding("tests[...]".into(), false)),
        ["require", ..] if chain.next == Some('(') => Some(finding("require()".into(), false)),
        [name, ..] if LEGACY_GLOBALS.contains(name) => Some(finding((*name).into(), false)),
        [name, ..] if ASYNC_NAMES.contains(name) => Some(finding((*name).into(), false)),
        ["_", _, ..] => Some(finding("_ (lodash)".into(), false)),
        [name, ..] if *name != "_" && LIBRARIES.contains(name) => {
            Some(finding((*name).into(), false))
        }
        _ => None,
    }
}

/// `(api, supported)` for a chain after `pm.`; `None` when fully supported.
fn classify_pm(rest: &[&str]) -> Option<(String, bool)> {
    let member = *rest.first()?;
    let path = |depth: usize| format!("pm.{}", rest[..depth.min(rest.len())].join("."));
    let unsupported = |depth: usize| Some((path(depth), false));
    match member {
        "sendRequest" => Some((path(1), true)),
        "test" | "expect" => None,
        "variables" if rest.get(1) == Some(&"replaceIn") => unsupported(2),
        m if SCOPE_MEMBERS.contains(&m) => match rest.get(1) {
            Some(method) if !SCOPE_METHODS.contains(method) => unsupported(2),
            _ => None,
        },
        "request" => match (rest.get(1), rest.get(2)) {
            (Some(&"headers"), Some(method)) if !HEADER_METHODS.contains(method) => unsupported(3),
            (Some(m), _) if !REQUEST_MEMBERS.contains(m) => unsupported(2),
            _ => None,
        },
        "response" => classify_response(rest).then(|| (path(rest.len()), false)),
        _ => unsupported(1),
    }
}

/// Whether a `pm.response…` chain uses something we lack.
fn classify_response(rest: &[&str]) -> bool {
    match rest.get(1) {
        None => false,
        Some(&"to") => {
            let bad_member = rest
                .get(2)
                .is_some_and(|m| !RESPONSE_TO_MEMBERS.contains(m));
            bad_member || rest.iter().any(|m| RESPONSE_UNSUPPORTED.contains(m))
        }
        Some(member) => !RESPONSE_MEMBERS.contains(member),
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$'
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Every member chain whose head is not itself a property (`a.pm.x` is skipped).
fn chains(code: &str) -> Vec<Chain> {
    let chars: Vec<char> = code.chars().collect();
    let mut found = Vec::new();
    let mut line = 1u32;
    let mut index = 0;
    let mut previous: Option<char> = None;
    while index < chars.len() {
        let c = chars[index];
        if c == '\n' {
            line += 1;
        }
        if !is_ident_start(c) {
            if !c.is_whitespace() {
                previous = Some(c);
            }
            index += 1;
            continue;
        }
        let start_line = line;
        let (parts, end, lines) = read_chain(&chars, index);
        line += lines;
        let next = chars[end..].iter().copied().find(|c| !c.is_whitespace());
        if previous != Some('.') {
            found.push(Chain {
                line: start_line,
                parts,
                next,
            });
        }
        previous = chars[..end]
            .iter()
            .rev()
            .copied()
            .find(|c| !c.is_whitespace());
        index = end;
    }
    found
}

/// Reads `ident(.ident)*` from `start`; returns the parts, the end and newlines crossed.
fn read_chain(chars: &[char], start: usize) -> (Vec<String>, usize, u32) {
    let mut parts = Vec::new();
    let mut index = start;
    let mut lines = 0u32;
    loop {
        let begin = index;
        while index < chars.len() && is_ident_char(chars[index]) {
            index += 1;
        }
        parts.push(chars[begin..index].iter().collect());
        let mut look = index;
        let mut crossed = 0u32;
        while look < chars.len() && chars[look].is_whitespace() {
            crossed += u32::from(chars[look] == '\n');
            look += 1;
        }
        if look >= chars.len() || chars[look] != '.' {
            return (parts, index, lines);
        }
        look += 1;
        while look < chars.len() && chars[look].is_whitespace() {
            crossed += u32::from(chars[look] == '\n');
            look += 1;
        }
        if look >= chars.len() || !is_ident_start(chars[look]) {
            return (parts, index, lines);
        }
        lines += crossed;
        index = look;
    }
}

/// Lexer state while blanking literals.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Code,
    Line,
    Block,
    Quote(char),
}

/// `script` with comments, strings and template literals replaced by spaces; line breaks
/// are kept so line numbers stay right.
fn blank_literals(script: &str) -> String {
    let chars: Vec<char> = script.chars().collect();
    let mut out = String::with_capacity(script.len());
    let mut mode = Mode::Code;
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        let next = chars.get(index + 1).copied();
        let (emit, new_mode, step) = match mode {
            Mode::Code => match (c, next) {
                ('/', Some('/')) => (' ', Mode::Line, 2),
                ('/', Some('*')) => (' ', Mode::Block, 2),
                ('"' | '\'' | '`', _) => (' ', Mode::Quote(c), 1),
                _ => (c, Mode::Code, 1),
            },
            Mode::Line if c == '\n' => ('\n', Mode::Code, 1),
            Mode::Line => (' ', Mode::Line, 1),
            Mode::Block if c == '*' && next == Some('/') => (' ', Mode::Code, 2),
            Mode::Quote(_) if c == '\\' => (' ', mode, 2),
            Mode::Quote(quote) if c == quote => (' ', Mode::Code, 1),
            Mode::Block | Mode::Quote(_) => (blank(Some(c)), mode, 1),
        };
        out.push(emit);
        if step == 2 {
            out.push(blank(next));
        }
        mode = new_mode;
        index += step;
    }
    out
}

fn blank(c: Option<char>) -> char {
    if c == Some('\n') {
        '\n'
    } else {
        ' '
    }
}

#[cfg(test)]
#[path = "scripts_tests.rs"]
mod tests;
