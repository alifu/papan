//! Limits and redaction suite for the script sandbox (plan 05 §4.3). Escapes: `script_sandbox.rs`.
//!
//! Each probe is a script a hostile collection could carry. The assertions pin what the
//! probe returned, so this file is also the record of how the sandbox answers each one.
//! The rule being checked: a probe is either unavailable or harmless — it never reaches
//! the host, never outlives its chain, never takes the app down.

use std::time::{Duration, Instant};

use papan_lib::script::{
    Layers, Limits, Script, ScriptContext, ScriptEngine, ScriptErrorKind, ScriptPhase,
    ScriptReport, ScriptRequest, VarLayer,
};

fn context() -> ScriptContext {
    let environment = VarLayer {
        values: [("token", "s3cret-token"), ("host", "api.test")]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        secrets: ["token".to_string()].into_iter().collect(),
    };
    ScriptContext::new(
        Layers {
            environment: Some(environment),
            ..Layers::default()
        },
        ScriptRequest {
            method: "GET".into(),
            url: "https://api.test/".into(),
            headers: Vec::new(),
            body: None,
        },
    )
}

fn quick() -> Limits {
    Limits {
        timeout: Duration::from_millis(400),
        ..Limits::default()
    }
}

fn run_in(ctx: &mut ScriptContext, source: &str, limits: &Limits) -> ScriptReport {
    let script = Script::new("probe", source);
    ScriptEngine::new().run(ScriptPhase::PreRequest, &[script], ctx, limits)
}

fn run(source: &str) -> ScriptReport {
    run_in(&mut context(), source, &Limits::default())
}

/// Console lines of a probe that must finish without an error.
fn lines(source: &str) -> Vec<String> {
    let report = run(source);
    assert_eq!(report.error, None, "{source}");
    report.console.into_iter().map(|line| line.text).collect()
}

fn kind_of(report: &ScriptReport) -> ScriptErrorKind {
    report.error.as_ref().expect("the probe must fail").kind
}

// ---------------------------------------------------------------- oversized arguments

#[test]
fn oversized_names_and_values_are_rejected_without_changing_anything() {
    let mut ctx = context();
    let report = run_in(
        &mut ctx,
        "const out = [];
         const attempt = (label, fn) => { try { fn(); out.push(label + '=ok'); } catch (e) { out.push(label + '!' + e.name); } };
         attempt('name128', () => pm.variables.set('n'.repeat(128), 'v'));
         attempt('name129', () => pm.variables.set('n'.repeat(129), 'v'));
         attempt('emptyName', () => pm.variables.set('', 'v'));
         attempt('controlName', () => pm.variables.set('a\\u0000b', 'v'));
         attempt('value64k', () => pm.variables.set('fits', 'v'.repeat(64 * 1024)));
         attempt('value64k1', () => pm.variables.set('big', 'v'.repeat(64 * 1024 + 1)));
         attempt('url', () => { pm.request.url = 'https://a.test/' + 'u'.repeat(64 * 1024); });
         attempt('headerValue', () => pm.request.headers.add({ key: 'X', value: 'v'.repeat(64 * 1024 + 1) }));
         attempt('headers201', () => { for (let i = 0; i < 201; i++) pm.request.headers.add({ key: 'H' + i, value: '1' }); });
         attempt('body5m', () => { pm.request.body = 'b'.repeat(5 * 1024 * 1024); });
         attempt('body5m1', () => { pm.request.body = 'b'.repeat(5 * 1024 * 1024 + 1); });
         attempt('btoaHuge', () => btoa('b'.repeat(5 * 1024 * 1024 + 1)));
         console.log(out.join(' '));",
        &Limits::default(),
    );
    assert_eq!(report.error, None);
    assert_eq!(
        report.console[0].text,
        "name128=ok name129!TypeError emptyName!TypeError controlName!TypeError value64k=ok \
         value64k1!TypeError url!TypeError headerValue!TypeError headers201!TypeError \
         body5m=ok body5m1!TypeError btoaHuge!TypeError"
    );
    assert_eq!(ctx.layers.runtime.values.len(), 2);
    assert_eq!(ctx.request.url, "https://api.test/");
    assert_eq!(ctx.request.headers.len(), 200);
    assert_eq!(ctx.request.body.map(|b| b.len()), Some(5 * 1024 * 1024));
}

#[test]
fn a_script_cannot_grow_host_memory_through_variables() {
    let mut ctx = context();
    let report = run_in(
        &mut ctx,
        "let stored = 0, refused = 0; const value = 'v'.repeat(64 * 1024);
         for (let i = 0; i < 3000; i++) { try { pm.globals.set('k' + i, value); stored++; } catch (e) { refused++; } }
         for (let i = 0; i < 3000; i++) { try { pm.variables.set('k' + i, ''); } catch (e) { refused++; } }
         console.log(stored, refused);",
        &Limits::default(),
    );
    assert_eq!(report.error, None);
    // 4 MiB of values per scope, 1000 names per scope.
    assert_eq!(report.console[0].text, "63 4937");
    assert_eq!(ctx.layers.globals.values.len(), 63);
    assert_eq!(ctx.layers.runtime.values.len(), 1000);
}

#[test]
fn console_output_and_test_results_are_capped() {
    let report = run("for (let i = 0; i < 1500; i++) console.log('line ' + i);
         console.log('x'.repeat(100000));
         let recorded = 0;
         try { for (let i = 0; i < 600; i++) { pm.test('t' + i, () => {}); recorded++; } }
         catch (e) { pm.variables.set('stopped', e.message); }
         pm.variables.set('recorded', recorded);");
    assert_eq!(report.error, None);
    assert_eq!(report.console.len(), 1000);
    assert!(report.console_truncated);
    assert_eq!(report.tests.len(), 500);
    let recorded = report
        .changes
        .iter()
        .find(|c| c.name == "recorded")
        .unwrap();
    assert_eq!(recorded.value.as_deref(), Some("500"));

    let long = run("console.log('é'.repeat(100000))");
    assert!(long.console[0].text.len() <= 4 * 1024 + 3);
}

// ---------------------------------------------------------------- limits

#[test]
fn a_runaway_loop_is_stopped_and_cannot_catch_its_way_out() {
    let mut ctx = context();
    let started = Instant::now();
    let report = run_in(
        &mut ctx,
        "pm.variables.set('before', '1');
         try { while (true) {} } catch (e) { pm.variables.set('caught', '1'); }
         finally { pm.variables.set('finally', '1'); }
         pm.variables.set('after', '1');",
        &quick(),
    );
    assert_eq!(kind_of(&report), ScriptErrorKind::Timeout);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    let kept: Vec<_> = ctx.layers.runtime.values.keys().cloned().collect();
    assert_eq!(kept, vec!["before"]);
    assert_eq!(report.error.unwrap().script.as_deref(), Some("probe"));
}

#[test]
fn the_default_limits_are_five_seconds_and_32_mib() {
    let limits = Limits::default();
    assert_eq!(limits.timeout, Duration::from_secs(5));
    assert_eq!(limits.memory_bytes, 32 * 1024 * 1024);
    assert_eq!(limits.max_send_requests, 5);

    let started = Instant::now();
    let report = run("for (;;) {}");
    let elapsed = started.elapsed();
    assert_eq!(kind_of(&report), ScriptErrorKind::Timeout);
    assert!(
        elapsed >= Duration::from_secs(5) && elapsed < Duration::from_secs(8),
        "{elapsed:?}"
    );
}

#[test]
fn slow_paths_inside_the_engine_are_interrupted_too() {
    for (label, source) in [
        ("regex backtracking", "/^(a+)+$/.test('a'.repeat(64) + 'b')"),
        ("promise chain", "(function again() { Promise.resolve().then(again); })()"),
        ("inside pm.test", "pm.test('spin', () => { for (;;) {} })"),
        ("getter on the thrown error", "const e = new Error('x'); Object.defineProperty(e, 'name', { get() { for (;;) {} } }); throw e;"),
        ("sort comparator", "[3, 2, 1].sort(() => { for (;;) {} })"),
        ("generated code", "new Function('for (;;) {}')()"),
    ] {
        let started = Instant::now();
        let report = run_in(&mut context(), source, &quick());
        let error = report.error.unwrap_or_else(|| panic!("{label} finished"));
        assert!(started.elapsed() < Duration::from_secs(3), "{label}: {:?}", started.elapsed());
        let expected = match label {
            // The loop runs after the script already failed, while the error is described.
            "getter on the thrown error" => ScriptErrorKind::Exception,
            _ => ScriptErrorKind::Timeout,
        };
        assert_eq!(error.kind, expected, "{label}: {}", error.message);
        assert!(report.tests.is_empty(), "{label}");
    }
}

#[test]
fn a_memory_bomb_ends_the_script_not_the_app() {
    for (label, source) in [
        (
            "array of strings",
            "const hoard = []; for (;;) hoard.push('x'.repeat(1024 * 1024) + hoard.length);",
        ),
        ("string doubling", "let s = 'x'; for (;;) s += s;"),
        (
            "typed array",
            "const hoard = []; for (;;) hoard.push(new Uint8Array(8 * 1024 * 1024));",
        ),
        ("one huge allocation", "new Array(1e9).fill(0)"),
    ] {
        let started = Instant::now();
        let report = run(source);
        let error = report.error.unwrap_or_else(|| panic!("{label} finished"));
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "{label}: {:?}",
            started.elapsed()
        );
        // QuickJS refuses a string past its length limit before the heap cap is reached.
        let expected = match label {
            "string doubling" => (ScriptErrorKind::Exception, "InternalError: string too long"),
            _ => (
                ScriptErrorKind::Memory,
                "The script used more memory than allowed.",
            ),
        };
        assert_eq!((error.kind, error.message.as_str()), expected, "{label}");
    }
    // The process is fine and the next chain starts from an empty heap.
    assert_eq!(lines("console.log('alive')"), vec!["alive"]);
}

#[test]
fn a_bomb_that_catches_its_own_errors_still_ends() {
    let started = Instant::now();
    let report = run_in(
        &mut context(),
        "const hoard = []; for (;;) { try { hoard.push('x'.repeat(1024 * 1024) + hoard.length); } catch (e) {} }",
        &quick(),
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(matches!(
        kind_of(&report),
        ScriptErrorKind::Timeout | ScriptErrorKind::Memory
    ));
}

#[test]
fn runaway_recursion_hits_the_stack_cap() {
    let report = run("function down(n) { return down(n + 1) + 1; } down(0);");
    assert_eq!(kind_of(&report), ScriptErrorKind::StackOverflow);

    // It is an ordinary RangeError for a script that wants to handle it.
    assert_eq!(
        lines("function down() { down(); } try { down(); } catch (e) { console.log(e.name); }"),
        vec!["RangeError"]
    );
    // Deep recursion through native code (JSON, nested arrays) is bounded as well.
    let nested = run("let a = []; for (let i = 0; i < 1e6; i++) a = [a]; JSON.stringify(a);");
    assert!(nested.error.is_some());
}

#[test]
fn cancelling_stops_a_running_script() {
    let engine = ScriptEngine::new();
    let cancel = engine.cancel_flag();
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        cancel.cancel();
    });
    let started = Instant::now();
    let mut ctx = context();
    let script = Script::new("probe", "for (;;) {}");
    let report = engine.run(
        ScriptPhase::PreRequest,
        &[script],
        &mut ctx,
        &Limits::default(),
    );
    canceller.join().unwrap();
    assert_eq!(kind_of(&report), ScriptErrorKind::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn odd_sources_fail_cleanly() {
    for source in ["\0", "var a = '\0';"] {
        let error = run(source).error.unwrap();
        assert_eq!(error.kind, ScriptErrorKind::Exception);
        assert_eq!(error.message, "A script cannot contain a NUL character.");
    }
    for source in ["\u{2028}}", "#!shebang", "}", "/*", "`"] {
        let error = run(source).error.unwrap();
        assert!(
            error.message.starts_with("SyntaxError"),
            "{source:?}: {}",
            error.message
        );
    }
    // Closing the wrapper early is just more of the same script in the same sandbox.
    let mut ctx = context();
    let report = run_in(
        &mut ctx,
        "})(); pm.variables.set('x', typeof require); (function () {",
        &Limits::default(),
    );
    assert_eq!(report.error, None);
    assert_eq!(ctx.layers.runtime.values["x"], "undefined");
}

// ---------------------------------------------------------------- redaction

#[test]
fn secrets_are_masked_everywhere_in_the_report() {
    let mut ctx = context();
    let report = run_in(
        &mut ctx,
        "const t = pm.environment.get('token');
         console.log(t); console.error({ nested: [t] }); console.warn(new Error(t));
         pm.test('token is ' + t, () => pm.expect(t).to.equal('other'));
         pm.test(t, () => { throw new Error('bad ' + t); });
         pm.globals.set('copy', 'Bearer ' + t);
         pm.environment.set('token', 'rotated-token-2');
         console.log(pm.environment.get('token'));
         pm.environment.unset('host');
         throw new TypeError('gave up on ' + t + ' and ' + pm.environment.get('token'));",
        &Limits::default(),
    );
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("s3cret-token"), "{json}");
    assert!(!json.contains("rotated-token-2"), "{json}");
    assert_eq!(json.matches("[REDACTED]").count(), 11, "{json}");
    assert_eq!(
        report.error.unwrap().message,
        "TypeError: gave up on [REDACTED] and [REDACTED]"
    );
    let token = report.changes.iter().find(|c| c.name == "token").unwrap();
    assert_eq!(
        (token.value.clone(), token.secret, token.removed),
        (None, true, false)
    );
    // The host still holds the real value for the vault.
    let kept = ctx
        .changes()
        .into_iter()
        .find(|c| c.name == "token")
        .unwrap();
    assert_eq!(kept.value.as_deref(), Some("rotated-token-2"));
}

#[test]
fn redaction_matches_the_value_not_its_transformations() {
    // Known limit, recorded here: a script that re-encodes a secret before printing it
    // defeats value matching. Scripts are the user's own code and may read secrets by
    // design (signing); the caller's redactor can add the encodings it knows about.
    let report = run("console.log(btoa(pm.environment.get('token')))");
    assert_eq!(report.console[0].text, "czNjcmV0LXRva2Vu");
}
