//! POSIX shell quoting for the exporter. Only plain `'…'` is used: it carries every
//! character literally in every POSIX shell (dash has no `$'…'`). Control characters other
//! than newline, tab and carriage return are refused before a command is returned.

/// `'…'`, with `'\''` for a quote.
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// `text` as is when it needs no quoting, else [`quote`].
pub fn bare_or_quoted(text: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "@%+=:,./_-".contains(c);
    if !text.is_empty() && text.chars().all(safe) {
        text.to_string()
    } else {
        quote(text)
    }
}
