//! The host-side watchdog and output redaction of the script sandbox (plan 05 §4.3).
//!
//! A chain that the engine cannot interrupt (a native loop) is abandoned at its deadline;
//! redaction runs before the output limits, so a cut never leaves a secret prefix.

use std::time::{Duration, Instant};

use papan_lib::script::{
    Layers, Limits, Script, ScriptContext, ScriptEngine, ScriptErrorKind, ScriptPhase,
    ScriptReport, ScriptRequest, StrayThreads, VarLayer, MAX_STRAY_THREADS, WATCHDOG_GRACE,
};

fn context() -> ScriptContext {
    let environment = VarLayer {
        values: [("token".to_string(), "s3cret-token".to_string())]
            .into_iter()
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

fn run_in(ctx: &mut ScriptContext, source: &str, limits: &Limits) -> ScriptReport {
    let script = Script::new("probe", source);
    ScriptEngine::new().run(ScriptPhase::PreRequest, &[script], ctx, limits)
}

fn kind_of(report: &ScriptReport) -> ScriptErrorKind {
    report.error.as_ref().expect("the probe must fail").kind
}

// ---------------------------------------------------------------- host-side watchdog

/// A bound native function called by `map`: the engine never reaches an interrupt check.
const NATIVE_LOOP: &str = "pm.variables.set('before', '1');
     const s = 'a'.repeat(8e6);
     new Array(1e6).fill(s).map(String.prototype.lastIndexOf.bind(s, 'b'))";
const SLACK: Duration = Duration::from_millis(500);

#[test]
fn a_native_loop_that_ignores_the_interrupt_is_abandoned_at_the_deadline() {
    let mut ctx = context();
    let limits = Limits {
        timeout: Duration::from_millis(400),
        ..Limits::default()
    };
    let started = Instant::now();

    let report = run_in(&mut ctx, NATIVE_LOOP, &limits);

    assert!(started.elapsed() < limits.timeout + WATCHDOG_GRACE + SLACK);
    assert_eq!(kind_of(&report), ScriptErrorKind::Timeout);
    assert!(
        ctx.changes().is_empty(),
        "an abandoned chain changes nothing"
    );
}

#[test]
fn cancel_returns_promptly_from_a_native_loop() {
    let engine = ScriptEngine::new();
    let flag = engine.cancel_flag();
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        flag.cancel();
    });
    let started = Instant::now();

    let report = engine.run(
        ScriptPhase::PreRequest,
        &[Script::new("probe", NATIVE_LOOP)],
        &mut context(),
        &Limits::default(),
    );

    canceller.join().unwrap();
    assert!(started.elapsed() < Duration::from_millis(100) + WATCHDOG_GRACE + SLACK);
    assert_eq!(kind_of(&report), ScriptErrorKind::Cancelled);
}

#[test]
fn abandoned_threads_are_counted_and_new_chains_refused_at_the_cap() {
    let strays = StrayThreads::default();
    let engine = ScriptEngine::new().with_strays(strays.clone());
    let limits = Limits {
        timeout: Duration::from_millis(30),
        ..Limits::default()
    };
    // Long enough to outlive the test's own chains (4 x (timeout + grace) = about 1.1 s; the
    // Windows runner scans several times faster than a Mac), short enough to end on its own.
    let slow = "const s = 'a'.repeat(8e6);
                new Array(200).fill(s).map(String.prototype.lastIndexOf.bind(s, 'b'))";
    let probe = [Script::new("probe", slow)];

    for _ in 0..MAX_STRAY_THREADS {
        let report = engine.run(ScriptPhase::PreRequest, &probe, &mut context(), &limits);
        assert_eq!(kind_of(&report), ScriptErrorKind::Timeout);
    }
    assert_eq!(strays.count(), MAX_STRAY_THREADS);
    let refused = engine.run(ScriptPhase::PreRequest, &probe, &mut context(), &limits);
    assert_eq!(kind_of(&refused), ScriptErrorKind::Internal);

    // A late thread only leaves the count; it reports to nobody.
    let waited = Instant::now();
    while strays.count() > 0 && waited.elapsed() < Duration::from_secs(120) {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(strays.count(), 0);
}

// ---------------------------------------------------------------- redact, then truncate

#[test]
fn a_secret_cut_by_the_output_limit_leaves_no_prefix() {
    let report = run_in(
        &mut context(),
        "const t = pm.environment.get('token');
         const pad = 'x'.repeat(4096 - 6);
         console.log(pad + t);
         pm.test(pad + t, () => { throw new Error(pad + t); });",
        &Limits::default(),
    );

    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("s3cr"), "a secret prefix leaked");
    assert!(report.console[0].text.len() <= 4096 + '…'.len_utf8());
    assert!(report.tests[0].name.len() <= 4096 + '…'.len_utf8());
}

#[test]
fn a_json_escaped_secret_is_masked() {
    let mut ctx = context();
    let env = ctx.layers.environment.as_mut().unwrap();
    env.values.insert("token".into(), "pa\"ss\\word\n1".into());
    let mut ctx = ScriptContext::new(ctx.layers, ctx.request);

    let report = run_in(
        &mut ctx,
        "console.log(JSON.stringify({ t: pm.environment.get('token') }));",
        &Limits::default(),
    );

    assert_eq!(report.console[0].text, r#"{"t":"[REDACTED]"}"#);
}
