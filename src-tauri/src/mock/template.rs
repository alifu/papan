//! Handlebars templating with Mockoon-style helpers (plan 11 §4). Helpers read the
//! request from the render data (`request`), so one registry serves every mock.

use std::sync::OnceLock;

use handlebars::{
    BlockContext, Context, Handlebars, Helper, HelperResult, Output, RenderContext, RenderError,
    RenderErrorReason, Renderable,
};
use rand::Rng;
use serde_json::{json, Value};

use super::model::MAX_BODY_BYTES;
use super::request::{json_path, RequestCtx};

/// `repeat` and `lorem` are capped so one template cannot exhaust memory.
const MAX_REPEAT: u64 = 1_000;
const MAX_LOREM_WORDS: u64 = 500;
/// All `repeat` iterations of one render share this budget, so nesting cannot multiply.
const REPEAT_BUDGET: u64 = 10_000;
/// Deeper nesting is refused before rendering: the engine recurses per level.
pub const MAX_BLOCK_DEPTH: usize = 16;
pub const MAX_EXPR_DEPTH: usize = 16;

thread_local! {
    static REPEATS_LEFT: std::cell::Cell<u64> = const { std::cell::Cell::new(REPEAT_BUDGET) };
}
const LOREM: [&str; 16] = [
    "lorem",
    "ipsum",
    "dolor",
    "sit",
    "amet",
    "consectetur",
    "adipiscing",
    "elit",
    "sed",
    "do",
    "eiusmod",
    "tempor",
    "incididunt",
    "ut",
    "labore",
    "magna",
];

type Out<'a> = &'a mut dyn Output;

fn fail(message: impl Into<String>) -> RenderError {
    RenderErrorReason::Other(message.into()).into()
}

fn param<'a>(h: &'a Helper<'_>, index: usize) -> Option<&'a Value> {
    h.param(index).map(|p| p.value())
}

fn text_param(h: &Helper<'_>, index: usize) -> String {
    match param(h, index) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn number_param(h: &Helper<'_>, index: usize) -> Option<f64> {
    param(h, index).and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
}

fn request_of(ctx: &Context) -> &Value {
    ctx.data().get("request").unwrap_or(&Value::Null)
}

fn write_value(out: Out<'_>, value: &Value) -> HelperResult {
    match value {
        Value::Null => Ok(()),
        Value::String(text) => out.write(text).map_err(RenderError::from),
        other => out.write(&other.to_string()).map_err(RenderError::from),
    }
}

/// Looks `name` up in a `[{name,value}]`-style object of the request data, else uses the
/// default (second parameter).
fn lookup_helper(
    group: &'static str,
) -> impl Fn(
    &Helper<'_>,
    &Handlebars<'_>,
    &Context,
    &mut RenderContext<'_, '_>,
    Out<'_>,
) -> HelperResult
       + Send
       + Sync
       + 'static {
    move |h, _, ctx, _, out| {
        let name = text_param(h, 0);
        let found = request_of(ctx).get(group).and_then(|g| g.get(&name));
        match found.or_else(|| param(h, 1)) {
            Some(value) => write_value(out, value),
            None => Ok(()),
        }
    }
}

fn body_helper(
    h: &Helper<'_>,
    _: &Handlebars<'_>,
    ctx: &Context,
    _: &mut RenderContext<'_, '_>,
    out: Out<'_>,
) -> HelperResult {
    let path = text_param(h, 0);
    let request = request_of(ctx);
    let found = match request.get("bodyJson").filter(|v| !v.is_null()) {
        Some(json) => json_path(json, &path),
        None => request.get("form").and_then(|f| f.get(&path)),
    };
    match found.or_else(|| param(h, 1)) {
        Some(value) => write_value(out, value),
        None => Ok(()),
    }
}

fn field_helper(
    field: &'static str,
) -> impl Fn(
    &Helper<'_>,
    &Handlebars<'_>,
    &Context,
    &mut RenderContext<'_, '_>,
    Out<'_>,
) -> HelperResult
       + Send
       + Sync
       + 'static {
    move |_, _, ctx, _, out| match request_of(ctx).get(field) {
        Some(value) => write_value(out, value),
        None => Ok(()),
    }
}

fn base_url(
    h: &Helper<'_>,
    _: &Handlebars<'_>,
    ctx: &Context,
    _: &mut RenderContext<'_, '_>,
    out: Out<'_>,
) -> HelperResult {
    let _ = h;
    let host = request_of(ctx)
        .get("host")
        .and_then(Value::as_str)
        .unwrap_or("");
    out.write(&format!("http://{host}"))
        .map_err(RenderError::from)
}

fn uuid_helper(
    _: &Helper<'_>,
    _: &Handlebars<'_>,
    _: &Context,
    _: &mut RenderContext<'_, '_>,
    out: Out<'_>,
) -> HelperResult {
    out.write(&uuid::Uuid::new_v4().to_string())
        .map_err(RenderError::from)
}

fn int_helper(
    h: &Helper<'_>,
    _: &Handlebars<'_>,
    _: &Context,
    _: &mut RenderContext<'_, '_>,
    out: Out<'_>,
) -> HelperResult {
    let low = number_param(h, 0).unwrap_or(0.0) as i64;
    let high = number_param(h, 1).unwrap_or(100.0) as i64;
    let (low, high) = if low <= high {
        (low, high)
    } else {
        (high, low)
    };
    let value = rand::rng().random_range(low..=high);
    out.write(&value.to_string()).map_err(RenderError::from)
}

fn float_helper(
    h: &Helper<'_>,
    _: &Handlebars<'_>,
    _: &Context,
    _: &mut RenderContext<'_, '_>,
    out: Out<'_>,
) -> HelperResult {
    let low = number_param(h, 0).unwrap_or(0.0);
    let high = number_param(h, 1).unwrap_or(1.0);
    if !(low.is_finite() && high.is_finite() && (high - low).is_finite()) {
        return Err(fail("float needs finite bounds"));
    }
    let (low, high) = if low <= high {
        (low, high)
    } else {
        (high, low)
    };
    let value = if low == high {
        low
    } else {
        rand::rng().random_range(low..high)
    };
    out.write(&format!("{value:.2}")).map_err(RenderError::from)
}

fn boolean_helper(
    _: &Helper<'_>,
    _: &Handlebars<'_>,
    _: &Context,
    _: &mut RenderContext<'_, '_>,
    out: Out<'_>,
) -> HelperResult {
    out.write(if rand::rng().random_bool(0.5) {
        "true"
    } else {
        "false"
    })
    .map_err(RenderError::from)
}

fn one_of(
    h: &Helper<'_>,
    _: &Handlebars<'_>,
    _: &Context,
    _: &mut RenderContext<'_, '_>,
    out: Out<'_>,
) -> HelperResult {
    let choices: Vec<&Value> = h.params().iter().map(|p| p.value()).collect();
    if choices.is_empty() {
        return Ok(());
    }
    // A single array argument is also accepted: `{{oneOf (array 1 2)}}` style data.
    let pick = choices[rand::rng().random_range(0..choices.len())];
    write_value(out, pick)
}

fn lorem(
    h: &Helper<'_>,
    _: &Handlebars<'_>,
    _: &Context,
    _: &mut RenderContext<'_, '_>,
    out: Out<'_>,
) -> HelperResult {
    let words = number_param(h, 0)
        .map_or(5, |n| n.max(0.0) as u64)
        .min(MAX_LOREM_WORDS);
    let mut rng = rand::rng();
    let text: Vec<&str> = (0..words)
        .map(|_| LOREM[rng.random_range(0..LOREM.len())])
        .collect();
    out.write(&text.join(" ")).map_err(RenderError::from)
}

fn now(
    _: &Helper<'_>,
    _: &Handlebars<'_>,
    _: &Context,
    _: &mut RenderContext<'_, '_>,
    out: Out<'_>,
) -> HelperResult {
    let stamp = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|e| fail(e.to_string()))?;
    out.write(&stamp).map_err(RenderError::from)
}

fn timestamp(
    _: &Helper<'_>,
    _: &Handlebars<'_>,
    _: &Context,
    _: &mut RenderContext<'_, '_>,
    out: Out<'_>,
) -> HelperResult {
    let millis = time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000;
    out.write(&millis.to_string()).map_err(RenderError::from)
}

fn math(
    op: &'static str,
) -> impl Fn(
    &Helper<'_>,
    &Handlebars<'_>,
    &Context,
    &mut RenderContext<'_, '_>,
    Out<'_>,
) -> HelperResult
       + Send
       + Sync
       + 'static {
    move |h, _, _, _, out| {
        let a = number_param(h, 0).ok_or_else(|| fail(format!("{op}: first number is missing")))?;
        let b =
            number_param(h, 1).ok_or_else(|| fail(format!("{op}: second number is missing")))?;
        let result = match op {
            "add" => a + b,
            "subtract" => a - b,
            "multiply" => a * b,
            _ if b == 0.0 => return Err(fail("divide: division by zero")),
            _ => a / b,
        };
        let text = if result.fract() == 0.0 && result.abs() < 1e15 {
            format!("{}", result as i64)
        } else {
            result.to_string()
        };
        out.write(&text).map_err(RenderError::from)
    }
}

fn case(
    kind: &'static str,
) -> impl Fn(
    &Helper<'_>,
    &Handlebars<'_>,
    &Context,
    &mut RenderContext<'_, '_>,
    Out<'_>,
) -> HelperResult
       + Send
       + Sync
       + 'static {
    move |h, _, _, _, out| {
        let text = text_param(h, 0);
        let changed = match kind {
            "lowercase" => text.to_lowercase(),
            "uppercase" => text.to_uppercase(),
            _ => {
                let mut chars = text.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_uppercase().collect::<String>() + chars.as_str()
                })
            }
        };
        out.write(&changed).map_err(RenderError::from)
    }
}

fn repeat<'reg, 'rc>(
    h: &Helper<'rc>,
    registry: &'reg Handlebars<'reg>,
    ctx: &'rc Context,
    rc: &mut RenderContext<'reg, 'rc>,
    out: Out<'_>,
) -> HelperResult {
    let count = number_param(h, 0)
        .map_or(0, |n| n.max(0.0) as u64)
        .min(MAX_REPEAT);
    let Some(template) = h.template() else {
        return Ok(());
    };
    let granted = REPEATS_LEFT.with(|left| {
        let allowed = count.min(left.get());
        left.set(left.get() - allowed);
        allowed
    });
    if granted < count {
        return Err(fail("the template repeats too many times"));
    }
    for index in 0..count {
        let mut block = BlockContext::new();
        block.set_local_var("index", json!(index));
        rc.push_block(block);
        let rendered = template.render(registry, ctx, rc, out);
        rc.pop_block();
        rendered?;
    }
    Ok(())
}

fn registry() -> &'static Handlebars<'static> {
    static REGISTRY: OnceLock<Handlebars<'static>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut hb = Handlebars::new();
        hb.set_strict_mode(false);
        hb.register_escape_fn(handlebars::no_escape);
        hb.register_helper("urlParam", Box::new(lookup_helper("params")));
        hb.register_helper("queryParam", Box::new(lookup_helper("query")));
        hb.register_helper("header", Box::new(lookup_helper("headers")));
        hb.register_helper("cookie", Box::new(lookup_helper("cookies")));
        hb.register_helper("body", Box::new(body_helper));
        hb.register_helper("method", Box::new(field_helper("method")));
        hb.register_helper("path", Box::new(field_helper("path")));
        hb.register_helper("hostname", Box::new(field_helper("host")));
        hb.register_helper("ip", Box::new(field_helper("ip")));
        hb.register_helper("baseUrl", Box::new(base_url));
        hb.register_helper("uuid", Box::new(uuid_helper));
        hb.register_helper("int", Box::new(int_helper));
        hb.register_helper("float", Box::new(float_helper));
        hb.register_helper("boolean", Box::new(boolean_helper));
        hb.register_helper("oneOf", Box::new(one_of));
        hb.register_helper("lorem", Box::new(lorem));
        hb.register_helper("now", Box::new(now));
        hb.register_helper("timestamp", Box::new(timestamp));
        hb.register_helper("repeat", Box::new(repeat));
        for op in ["add", "subtract", "multiply", "divide"] {
            hb.register_helper(op, Box::new(math(op)));
        }
        for kind in ["lowercase", "uppercase", "capitalize"] {
            hb.register_helper(kind, Box::new(case(kind)));
        }
        hb
    })
}

/// The request as template data: `{{request.method}}`, and what the helpers read.
fn data_of(ctx: &RequestCtx) -> Value {
    let pairs = |items: &[(String, String)]| {
        Value::Object(
            items
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect(),
        )
    };
    json!({
        "request": {
            "method": ctx.method,
            "path": ctx.path,
            "ip": ctx.ip,
            "host": ctx.host,
            "query": pairs(&ctx.query),
            "headers": pairs(&ctx.headers),
            "cookies": pairs(&ctx.cookies),
            "params": pairs(&ctx.params),
            "form": pairs(&ctx.form),
            "bodyJson": ctx.body_json.clone().unwrap_or(Value::Null),
            "body": ctx.body_text,
        }
    })
}

/// Renders `template` for `ctx`. Bodies without `{{` skip the engine entirely.
pub fn render(template: &str, ctx: &RequestCtx) -> Result<String, String> {
    if !template.contains("{{") {
        return Ok(template.to_string());
    }
    check_depth(template)?;
    REPEATS_LEFT.with(|left| left.set(REPEAT_BUDGET));
    let mut sink = CappedWriter::default();
    registry()
        .render_template_to_write(template, &data_of(ctx), &mut sink)
        .map_err(|error| {
            if sink.overflowed {
                "the rendered body is over 1 MiB".to_string()
            } else {
                error.to_string()
            }
        })?;
    String::from_utf8(sink.bytes).map_err(|_| "the rendered body is not valid text".into())
}

/// Collects output and fails the render the moment it passes the body cap.
#[derive(Default)]
struct CappedWriter {
    bytes: Vec<u8>,
    overflowed: bool,
}

impl std::io::Write for CappedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len() + buf.len() > MAX_BODY_BYTES {
            self.overflowed = true;
            return Err(std::io::Error::other("body too large"));
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Refuses templates nested deeper than the engine can safely recurse.
pub fn check_depth(template: &str) -> Result<(), String> {
    let (mut blocks, mut parens, mut max_parens) = (0usize, 0usize, 0usize);
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' && bytes.get(i + 1) == Some(&b'{') {
            let mut at = i + 2;
            while matches!(bytes.get(at), Some(b'{' | b'~')) {
                at += 1;
            }
            match bytes.get(at) {
                Some(b'#') => {
                    blocks += 1;
                    if blocks > MAX_BLOCK_DEPTH {
                        return Err("blocks are nested too deeply".into());
                    }
                }
                Some(b'/') => blocks = blocks.saturating_sub(1),
                _ => {}
            }
            parens = 0;
            i = at;
            continue;
        }
        match bytes[i] {
            b'(' => {
                parens += 1;
                max_parens = max_parens.max(parens);
            }
            b')' => parens = parens.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
    if max_parens > MAX_EXPR_DEPTH {
        return Err("expressions are nested too deeply".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestCtx {
        RequestCtx {
            method: "POST".into(),
            path: "/users/42".into(),
            query: vec![("q".into(), "abc".into())],
            headers: vec![("x-role".into(), "admin".into())],
            cookies: vec![("sid".into(), "7".into())],
            params: vec![("id".into(), "42".into())],
            body_json: Some(json!({"user": {"name": "Ann"}, "tags": ["a", "b"]})),
            ip: "127.0.0.1".into(),
            host: "localhost:3000".into(),
            ..Default::default()
        }
    }

    fn out(template: &str) -> String {
        render(template, &ctx()).unwrap()
    }

    #[test]
    fn plain_text_is_returned_unchanged() {
        assert_eq!(out(r#"{"a": 1}"#), r#"{"a": 1}"#);
    }

    #[test]
    fn request_helpers_read_the_request() {
        assert_eq!(out("{{urlParam 'id'}}"), "42");
        assert_eq!(
            out("{{queryParam 'q'}}-{{queryParam 'zz' 'dflt'}}"),
            "abc-dflt"
        );
        assert_eq!(out("{{header 'x-role'}}"), "admin");
        assert_eq!(out("{{cookie 'sid'}}"), "7");
        assert_eq!(
            out("{{body 'user.name'}} {{body 'tags[1]'}} {{body 'nope' 'none'}}"),
            "Ann b none"
        );
        assert_eq!(
            out("{{method}} {{path}} {{hostname}} {{ip}} {{baseUrl}}"),
            "POST /users/42 localhost:3000 127.0.0.1 http://localhost:3000"
        );
    }

    #[test]
    fn output_is_not_html_escaped() {
        assert_eq!(
            out(r#"{"q": "{{queryParam 'q'}}", "x": "<a>"}"#),
            r#"{"q": "abc", "x": "<a>"}"#
        );
        let mut c = ctx();
        c.query = vec![("q".into(), "<b>&\"".into())];
        assert_eq!(render("{{queryParam 'q'}}", &c).unwrap(), "<b>&\"");
    }

    #[test]
    fn data_helpers_stay_in_range() {
        for _ in 0..50 {
            let n: i64 = out("{{int 5 10}}").parse().unwrap();
            assert!((5..=10).contains(&n));
            let f: f64 = out("{{float 1 2}}").parse().unwrap();
            assert!((1.0..=2.0).contains(&f));
        }
        assert_eq!(out("{{uuid}}").len(), 36);
        assert!(["true", "false"].contains(&out("{{boolean}}").as_str()));
        assert!(["a", "b"].contains(&out("{{oneOf 'a' 'b'}}").as_str()));
        assert_eq!(out("{{lorem 3}}").split(' ').count(), 3);
        assert!(out("{{now}}").contains('T'));
        assert!(out("{{timestamp}}").parse::<i64>().unwrap() > 1_700_000_000_000);
    }

    #[test]
    fn math_and_string_helpers() {
        assert_eq!(
            out("{{add 2 3}} {{subtract 5 1}} {{multiply 2 4}} {{divide 9 2}}"),
            "5 4 8 4.5"
        );
        assert_eq!(
            out("{{uppercase 'ab'}} {{lowercase 'AB'}} {{capitalize 'ab'}}"),
            "AB ab Ab"
        );
        assert!(render("{{divide 1 0}}", &ctx()).is_err());
    }

    #[test]
    fn repeat_has_an_index_and_a_cap() {
        assert_eq!(out("{{#repeat 3}}{{@index}},{{/repeat}}"), "0,1,2,");
        let big = out("{{#repeat 100000}}x{{/repeat}}");
        assert_eq!(big.len(), 1_000);
    }

    #[test]
    fn bad_templates_report_an_error() {
        assert!(render("{{#if}}", &ctx()).is_err());
        assert!(render("{{unclosed", &ctx()).is_err());
    }

    #[test]
    fn oversized_output_is_refused() {
        let huge = format!("{{{{#repeat 1000}}}}{}{{{{/repeat}}}}", "x".repeat(2_000));
        assert!(render(&huge, &ctx()).is_err());
    }

    #[test]
    fn nested_repeats_share_one_budget() {
        let nested =
            "{{#repeat 1000}}{{#repeat 1000}}{{#repeat 1000}}x{{/repeat}}{{/repeat}}{{/repeat}}";
        assert!(render(nested, &ctx()).is_err());
    }

    #[test]
    fn output_past_the_cap_stops_the_render() {
        let t = "{{#repeat 1000}}{{#repeat 5}}".to_string()
            + &"x".repeat(1000)
            + "{{/repeat}}{{/repeat}}";
        let error = render(&t, &ctx()).unwrap_err();
        assert!(error.contains("1 MiB"), "{error}");
    }

    #[test]
    fn deep_nesting_is_refused_before_rendering() {
        let blocks = "{{#if 1}}".repeat(3000) + &"{{/if}}".repeat(3000);
        assert!(render(&blocks, &ctx()).unwrap_err().contains("nested"));
        let parens = format!(
            "{{{{lowercase {}x{}}}}}",
            "(lowercase ".repeat(100),
            ")".repeat(100)
        );
        assert!(render(&parens, &ctx()).unwrap_err().contains("nested"));
        assert!(render("{{#if 1}}{{#if 1}}ok{{/if}}{{/if}}", &ctx()).is_ok());
    }

    #[test]
    fn float_with_non_finite_bounds_is_an_error_not_a_panic() {
        assert!(render("{{float 'NaN' 1}}", &ctx()).is_err());
        assert!(render("{{float '-1e308' '1e308'}}", &ctx()).is_err());
        assert!(render("{{float 1 2}}", &ctx()).is_ok());
    }
}
