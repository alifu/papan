use super::*;
use crate::script::context::ScriptContext;
use crate::script::report::ScriptPhase;

fn state() -> Rc<RefCell<HostState>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let host = HostState::new(
        ScriptPhase::PreRequest,
        ScriptContext::default(),
        None,
        deadline,
        0,
    );
    Rc::new(RefCell::new(host))
}

/// Evaluates `source` with the raw native function bound to `host`, which no user script
/// ever sees, and returns what the expression produced.
fn with_raw_host(source: &str) -> String {
    let state = state();
    let runtime = Runtime::new().unwrap();
    let context = Context::full(&runtime).unwrap();
    context.with(|ctx| {
        let shared = Rc::clone(&state);
        let host = Function::new(
            ctx.clone(),
            move |ctx: Ctx<'_>, args: Rest<Value<'_>>| -> rquickjs::Result<Option<String>> {
                host_call(&shared, &args.0).map_err(|m| Exception::throw_type(&ctx, &m))
            },
        )
        .unwrap();
        ctx.globals().set("host", host).unwrap();
        let wrapped = format!("(() => {{ try {{ return String({source}); }} catch (e) {{ return e.name + ': ' + e.message; }} }})()");
        ctx.eval::<String, _>(wrapped).unwrap()
    })
}

#[test]
fn the_native_function_takes_only_a_few_primitive_strings() {
    for (call, expected) in [
        ("host()", "TypeError: Invalid host call."),
        (
            "host('var.get', 'runtime', 'a', 'b', 'c')",
            "TypeError: Invalid host call.",
        ),
        ("host(1)", "TypeError: Host arguments must be strings."),
        (
            "host('var.get', 'runtime', 7)",
            "TypeError: Host arguments must be strings.",
        ),
        (
            "host('var.get', 'runtime', null)",
            "TypeError: Host arguments must be strings.",
        ),
        (
            "host('var.get', 'runtime', new String('a'))",
            "TypeError: Host arguments must be strings.",
        ),
        (
            "host('var.get', 'runtime', { toString() { throw new Error('ran'); } })",
            "TypeError: Host arguments must be strings.",
        ),
        (
            "host('var.get', 'runtime', Symbol('a'))",
            "TypeError: Host arguments must be strings.",
        ),
        (
            "host('var.get', 'runtime', '\\uDC00')",
            "TypeError: A string is not valid Unicode text.",
        ),
        ("host('constructor')", "TypeError: Unknown host call."),
        ("host('__proto__', 'a')", "TypeError: Unknown host call."),
        (
            "host('var.get', 'constructor', 'a')",
            "TypeError: Unknown variable scope.",
        ),
        ("host('var.set', 'runtime', 'a', 'b')", "undefined"),
        ("host('var.get', 'runtime', 'a')", "undefined"),
        ("typeof host.constructor('return this')().host", "function"),
    ] {
        assert_eq!(with_raw_host(call), expected, "{call}");
    }
}

#[test]
fn a_reentrant_host_call_is_refused_instead_of_panicking() {
    let state = state();
    let _held = state.borrow_mut();
    let runtime = Runtime::new().unwrap();
    let context = Context::full(&runtime).unwrap();
    context.with(|ctx| {
        let op = rquickjs::String::from_str(ctx.clone(), "req.get").unwrap();
        let reply = host_call(&state, &[op.into_value()]);
        assert_eq!(reply, Err("The host is busy.".to_string()));
    });
}

#[test]
fn script_line_finds_the_first_user_frame() {
    let stack =
        "    at _assert (prelude:70:13)\n    at <anonymous> (script:12:5)\n    at script:1:1";
    assert_eq!(script_line(stack), Some(12));
    assert_eq!(script_line("    at script (prelude:3:1)"), None);
    assert_eq!(script_line(""), None);
}

#[test]
fn the_cancel_flag_is_shared_between_clones() {
    let flag = CancelFlag::new();
    let clone = flag.clone();
    assert!(!clone.is_cancelled());
    flag.cancel();
    assert!(clone.is_cancelled());
}
