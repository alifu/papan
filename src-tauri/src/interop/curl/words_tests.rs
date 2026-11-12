use super::*;

fn words(text: &str) -> Vec<String> {
    split(text).expect("splits")
}

#[test]
fn splits_on_whitespace_and_joins_adjacent_quoted_parts() {
    assert_eq!(words("a  'b c'\"d\"e\tf\ng"), ["a", "b cde", "f", "g"]);
}

#[test]
fn empty_quotes_make_an_empty_word() {
    assert_eq!(words("a '' \"\""), ["a", "", ""]);
}

#[test]
fn backslash_newline_is_a_continuation_also_with_crlf() {
    assert_eq!(words("a \\\r\n b\\\nc"), ["a", "bc"]);
}

#[test]
fn double_quotes_unescape_only_shell_specials() {
    assert_eq!(words(r#""\$x \` \" \\ \n""#), [r#"$x ` " \ \n"#]);
}

#[test]
fn dollar_forms_and_backticks_stay_literal() {
    assert_eq!(
        words("$HOME \"$(id)\" `whoami` ${X}"),
        ["$HOME", "$(id)", "`whoami`", "${X}"]
    );
}

#[test]
fn ansi_c_quotes_decode_escapes() {
    assert_eq!(words(r"$'a\nb\t\x41\u00e9\101\'\\\q'"), ["a\nb\tAéA'\\\\q"]);
}

#[test]
fn unterminated_quotes_are_an_error() {
    for text in ["'a", "\"a", "$'a", "\"a\\"] {
        assert_eq!(split(text), Err(SplitError::UnterminatedQuote), "{text}");
    }
}

#[test]
fn odd_inputs_never_panic() {
    let samples = [
        "",
        "\\",
        "$",
        "$'",
        "$'\\",
        "$'\\x",
        "$'\\u{'",
        "$'\\U110000'",
        "$'\\777'",
        "é\\",
        "\"\\",
        "'''",
        "\u{0}\u{7f}",
        "\\\r",
        "$'\\x\u{1F600}'",
    ];
    for sample in samples {
        let _ = split(sample);
    }
    for byte in 0u8..=127 {
        let text: String = [char::from(byte), '\\', '\'', '"', '$'].iter().collect();
        let _ = split(&text);
    }
}
