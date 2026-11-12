//! The cURL options the importer knows (plan 03b §7).

/// What an option does to the imported request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opt {
    Method,
    Url,
    Header,
    /// `-d`: `@file` reads a file.
    Data,
    /// `--data-raw`: `@` is literal.
    DataRaw,
    /// `--data-binary`: `@file` reads a file as is.
    DataBinary,
    DataUrlencode,
    Json,
    Form,
    FormString,
    User,
    Digest,
    UserAgent,
    Referer,
    Cookie,
    Get,
    Head,
    Location,
    Insecure,
    MaxTime,
    Http1,
    Http2,
    /// Output, verbosity and the like: no effect on the request.
    Ignored,
    /// Warned about; the text explains where the setting lives instead.
    Warned(&'static str),
}

/// An option and whether it takes an argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    pub opt: Opt,
    pub takes_arg: bool,
}

const fn arg(opt: Opt) -> Spec {
    Spec {
        opt,
        takes_arg: true,
    }
}

const fn flag(opt: Opt) -> Spec {
    Spec {
        opt,
        takes_arg: false,
    }
}

const PROXY: &str = " (set the proxy in Network settings)";
const CERT: &str = " (set client certificates in Network settings)";
const CONFIG: &str = " (config files are not read)";
const NONE: &str = "";

/// Looks up a short option letter.
pub fn short(letter: char) -> Option<Spec> {
    Some(match letter {
        'X' => arg(Opt::Method),
        'H' => arg(Opt::Header),
        'd' => arg(Opt::Data),
        'F' => arg(Opt::Form),
        'u' => arg(Opt::User),
        'A' => arg(Opt::UserAgent),
        'e' => arg(Opt::Referer),
        'b' => arg(Opt::Cookie),
        'G' => flag(Opt::Get),
        'I' => flag(Opt::Head),
        'L' => flag(Opt::Location),
        'k' => flag(Opt::Insecure),
        'm' => arg(Opt::MaxTime),
        'o' | 'w' | 'D' | 'c' => arg(Opt::Ignored),
        's' | 'S' | 'v' | 'i' | 'f' | 'g' | 'N' | 'O' | '#' => flag(Opt::Ignored),
        'x' => arg(Opt::Warned(PROXY)),
        'U' => arg(Opt::Warned(PROXY)),
        'E' => arg(Opt::Warned(CERT)),
        'K' => arg(Opt::Warned(CONFIG)),
        'T' | 'r' | 'y' | 'Y' | 'z' => arg(Opt::Warned(NONE)),
        _ => return None,
    })
}

/// Looks up a long option name, without the leading `--`.
pub fn long(name: &str) -> Option<Spec> {
    Some(match name {
        "request" => arg(Opt::Method),
        "url" => arg(Opt::Url),
        "header" => arg(Opt::Header),
        "data" | "data-ascii" => arg(Opt::Data),
        "data-raw" => arg(Opt::DataRaw),
        "data-binary" => arg(Opt::DataBinary),
        "data-urlencode" => arg(Opt::DataUrlencode),
        "json" => arg(Opt::Json),
        "form" => arg(Opt::Form),
        "form-string" => arg(Opt::FormString),
        "user" => arg(Opt::User),
        "digest" => flag(Opt::Digest),
        "user-agent" => arg(Opt::UserAgent),
        "referer" => arg(Opt::Referer),
        "cookie" => arg(Opt::Cookie),
        "get" => flag(Opt::Get),
        "head" => flag(Opt::Head),
        "location" => flag(Opt::Location),
        "insecure" => flag(Opt::Insecure),
        "max-time" => arg(Opt::MaxTime),
        "http1.1" => flag(Opt::Http1),
        "http2" | "http2-prior-knowledge" => flag(Opt::Http2),
        "output" | "write-out" | "dump-header" | "cookie-jar" | "stderr" | "trace"
        | "trace-ascii" => arg(Opt::Ignored),
        "silent" | "show-error" | "verbose" | "include" | "fail" | "fail-with-body"
        | "compressed" | "globoff" | "no-buffer" | "progress-bar" | "remote-name" => {
            flag(Opt::Ignored)
        }
        "proxy" | "proxy-user" | "socks5" | "socks5-hostname" | "preproxy" => {
            arg(Opt::Warned(PROXY))
        }
        "noproxy" => arg(Opt::Warned(PROXY)),
        "cert" | "key" | "cert-type" | "key-type" | "pass" => arg(Opt::Warned(CERT)),
        "config" => arg(Opt::Warned(CONFIG)),
        "cacert" | "capath" | "resolve" | "connect-to" | "retry" | "connect-timeout"
        | "max-redirs" | "upload-file" | "limit-rate" | "range" | "interface" | "oauth2-bearer"
        | "aws-sigv4" => arg(Opt::Warned(NONE)),
        _ => return None,
    })
}
