use std::sync::Mutex;
use std::time::Duration;

use super::*;
use crate::script::context::{Layers, Scope, ScriptRequest, ScriptResponse, VarLayer};
use crate::script::report::ConsoleLevel;

fn layer(pairs: &[(&str, &str)]) -> VarLayer {
    VarLayer {
        values: pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        ..VarLayer::default()
    }
}

fn context() -> ScriptContext {
    ScriptContext::new(
        Layers {
            globals: layer(&[("shared", "global"), ("g", "G")]),
            collection: layer(&[("shared", "collection")]),
            environment: Some(layer(&[("shared", "env"), ("host", "api.test")])),
            runtime: VarLayer::default(),
        },
        ScriptRequest {
            method: "GET".into(),
            url: "https://api.test/items".into(),
            headers: vec![("Accept".into(), "application/json".into())],
            body: Some("{\"a\":1}".into()),
        },
    )
}

fn response() -> ScriptResponse {
    ScriptResponse {
        code: 201,
        status: "Created".into(),
        headers: vec![("Content-Type".into(), "application/json".into())],
        body: "{\"token\":\"abc\",\"items\":[1,2]}".into(),
        response_time_ms: 42,
    }
}

fn run_with(phase: ScriptPhase, sources: &[&str], ctx: &mut ScriptContext) -> ScriptReport {
    let scripts: Vec<Script> = sources
        .iter()
        .enumerate()
        .map(|(i, source)| Script::new(format!("script{i}"), *source))
        .collect();
    ScriptEngine::new().run(phase, &scripts, ctx, &Limits::default())
}

fn pre(source: &str, ctx: &mut ScriptContext) -> ScriptReport {
    run_with(ScriptPhase::PreRequest, &[source], ctx)
}

fn post(source: &str) -> ScriptReport {
    let mut ctx = context();
    ctx.response = Some(response());
    run_with(ScriptPhase::PostResponse, &[source], &mut ctx)
}

fn logged(report: &ScriptReport) -> Vec<&str> {
    assert_eq!(report.error, None);
    report.console.iter().map(|l| l.text.as_str()).collect()
}

#[test]
fn an_empty_chain_does_nothing() {
    let mut ctx = context();
    let report = run_with(ScriptPhase::PreRequest, &["", "  \n"], &mut ctx);
    assert_eq!(report.error, None);
    assert!(report.console.is_empty() && report.changes.is_empty());
    assert_eq!(ctx, context());
}

#[test]
fn scoped_getters_read_their_scope_and_variables_reads_by_priority() {
    let mut ctx = context();
    let report = pre(
        "console.log(pm.environment.get('shared'), pm.collectionVariables.get('shared'),
           pm.globals.get('shared'), pm.variables.get('shared'), pm.variables.get('g'));
         pm.variables.set('shared', 'runtime');
         console.log(pm.variables.get('shared'), pm.environment.get('shared'));
         console.log(pm.environment.has('host'), pm.environment.has('nope'),
           pm.environment.get('nope') === undefined);
         console.log(JSON.stringify(pm.variables.toObject()));",
        &mut ctx,
    );
    assert_eq!(
        logged(&report),
        vec![
            "env collection global env G",
            "runtime env",
            "true false true",
            r#"{"g":"G","host":"api.test","shared":"runtime"}"#,
        ]
    );
}

#[test]
fn values_are_stringified_and_changes_are_reported() {
    let mut ctx = context();
    let report = pre(
        "pm.environment.set('n', 42); pm.environment.set('o', {a: [1]});
         pm.globals.set('b', true); pm.collectionVariables.unset('shared');
         pm.environment.unset('never-there');",
        &mut ctx,
    );
    assert_eq!(report.error, None);
    let env = &ctx.layers.environment.as_ref().unwrap().values;
    assert_eq!(
        (env["n"].as_str(), env["o"].as_str()),
        ("42", r#"{"a":[1]}"#)
    );
    assert_eq!(ctx.layers.globals.values["b"], "true");
    let names: Vec<_> = report
        .changes
        .iter()
        .map(|c| (c.name.as_str(), c.removed))
        .collect();
    assert_eq!(
        names,
        vec![("b", false), ("shared", true), ("n", false), ("o", false)]
    );
    assert_eq!(ctx.changes().len(), 4);
}

#[test]
fn scripts_run_outer_to_inner_and_share_variables_but_not_locals() {
    let mut ctx = context();
    let report = run_with(
        ScriptPhase::PreRequest,
        &[
            "const local = 1; pm.variables.set('order', 'collection');",
            "const local = 2; pm.variables.set('order', pm.variables.get('order') + '>folder');",
            "pm.variables.set('order', pm.variables.get('order') + '>request');
             console.log(typeof local);",
        ],
        &mut ctx,
    );
    assert_eq!(logged(&report), vec!["undefined"]);
    assert_eq!(
        ctx.layers.runtime.values["order"],
        "collection>folder>request"
    );
}

#[test]
fn a_failing_script_stops_the_chain_but_keeps_earlier_changes() {
    let mut ctx = context();
    let report = run_with(
        ScriptPhase::PreRequest,
        &[
            "pm.variables.set('a', '1');",
            "pm.variables.set('b', '2');\n\nthrow new Error('boom');",
            "pm.variables.set('c', '3');",
        ],
        &mut ctx,
    );
    let error = report.error.expect("an error");
    assert_eq!(error.kind, ScriptErrorKind::Exception);
    assert_eq!(error.message, "Error: boom");
    assert_eq!(error.script.as_deref(), Some("script1"));
    assert_eq!(error.line, Some(3));
    let runtime: Vec<_> = ctx.layers.runtime.values.keys().cloned().collect();
    assert_eq!(runtime, vec!["a", "b"]);
}

#[test]
fn syntax_errors_and_thrown_values_are_reported() {
    let syntax = pre("let x = ;", &mut context()).error.unwrap();
    assert_eq!(syntax.kind, ScriptErrorKind::Exception);
    assert!(
        syntax.message.starts_with("SyntaxError"),
        "{}",
        syntax.message
    );
    assert_eq!(
        pre("throw 'text'", &mut context()).error.unwrap().message,
        "Uncaught text"
    );
    assert_eq!(
        pre("throw {code: 7}", &mut context())
            .error
            .unwrap()
            .message,
        r#"Uncaught {"code":7}"#
    );
    let big = "/".repeat(MAX_SCRIPT_BYTES + 1);
    let too_big = pre(&big, &mut context()).error.unwrap();
    assert_eq!(too_big.script.as_deref(), Some("script0"));
}

#[test]
fn the_pre_request_script_can_change_the_request() {
    let mut ctx = context();
    let report = pre(
        "console.log(pm.request.method, pm.request.url, pm.request.headers.get('accept'),
           pm.request.body.raw, String(pm.request.body));
         pm.request.method = 'post';
         pm.request.url = pm.request.url + '?page=2';
         pm.request.headers.add({key: 'X-Trace', value: 1});
         pm.request.headers.upsert({key: 'accept', value: 'text/plain'});
         pm.request.headers.add('X-Gone', 'x'); pm.request.headers.remove('x-gone');
         pm.request.body.raw = JSON.stringify({signed: btoa('a:b')});
         console.log(pm.request.headers.has('X-Trace'), JSON.stringify(pm.request.headers.toObject()));",
        &mut ctx,
    );
    assert_eq!(
        logged(&report),
        vec![
            r#"GET https://api.test/items application/json {"a":1} {"a":1}"#,
            r#"true {"X-Trace":"1","accept":"text/plain"}"#,
        ]
    );
    assert_eq!(
        ctx.request,
        ScriptRequest {
            method: "post".into(),
            url: "https://api.test/items?page=2".into(),
            headers: vec![
                ("X-Trace".into(), "1".into()),
                ("accept".into(), "text/plain".into())
            ],
            body: Some(r#"{"signed":"YTpi"}"#.into()),
        }
    );
}

#[test]
fn the_body_can_be_replaced_or_removed_and_bad_changes_throw() {
    let mut ctx = context();
    let report = pre(
        "pm.request.body = 'plain'; const one = pm.request.body.raw;
         pm.request.body.update({raw: 'two'}); const two = pm.request.body.raw;
         pm.request.body = null;
         console.log(one, two, pm.request.body.raw === undefined);
         try { pm.request.headers.add({key: 'Bad Name', value: 'x'}); } catch (e) { console.log(e.name, e.message); }
         try { pm.request.url = 'http://a/\\r\\nX: y'; } catch (e) { console.log(e.name); }",
        &mut ctx,
    );
    assert_eq!(
        logged(&report),
        vec![
            "plain two true",
            "TypeError A header name is not a valid HTTP header name.",
            "TypeError",
        ]
    );
    assert_eq!(ctx.request.body, None);
    assert_eq!(ctx.request.url, "https://api.test/items");
}

#[test]
fn environment_set_without_an_environment_warns_and_stays_in_runtime() {
    let mut ctx = ScriptContext::default();
    let report = pre(
        "pm.environment.set('token', 't'); console.log(pm.variables.get('token'), pm.environment.get('token'));",
        &mut ctx,
    );
    assert_eq!(report.console[0].level, ConsoleLevel::Warn);
    assert_eq!(report.console[1].text, "t undefined");
    assert_eq!(ctx.changes()[0].scope, Scope::Runtime);
}

#[test]
fn base64_helpers_are_global() {
    let report = pre(
        "console.log(btoa('user:pass'), atob('dXNlcjpwYXNz'));
         try { btoa('☃'); } catch (e) { console.log(e.name); }",
        &mut context(),
    );
    assert_eq!(logged(&report), vec!["dXNlcjpwYXNz user:pass", "TypeError"]);
}

#[test]
fn secret_values_never_reach_the_report() {
    let mut ctx = context();
    let env = ctx.layers.environment.as_mut().unwrap();
    env.values.insert("password".into(), "hunter2".into());
    env.secrets.insert("password".into());
    let mut ctx = ScriptContext::new(ctx.layers, ctx.request);
    ctx.extra_secrets = vec!["basic-auth-pass".into()];
    let redactor: crate::script::Redactor = Arc::new(|text: &str| text.replace("hunter%32", "*"));
    let engine = ScriptEngine::new().with_redactor(redactor);
    let script = Script::new(
        "request",
        "const p = pm.environment.get('password');
         console.log('pw', p, 'basic-auth-pass', 'hunter%32');
         pm.test('leak ' + p, () => pm.expect(p).to.equal('x'));
         pm.environment.set('password', 'rotated-9');
         pm.environment.set('copy', p);
         console.log(pm.environment.get('password'));
         throw new Error('failed with ' + p);",
    );
    let report = engine.run(
        ScriptPhase::PreRequest,
        &[script],
        &mut ctx,
        &Limits::default(),
    );

    let json = serde_json::to_string(&report).unwrap();
    for secret in ["hunter2", "rotated-9", "basic-auth-pass", "hunter%32"] {
        assert!(!json.contains(secret), "{secret} leaked in {json}");
    }
    assert_eq!(report.console[0].text, "pw [REDACTED] [REDACTED] *");
    let password = report
        .changes
        .iter()
        .find(|c| c.name == "password")
        .unwrap();
    assert_eq!((password.value.as_deref(), password.secret), (None, true));
    // Persistence still gets the real value.
    let change = ctx
        .changes()
        .into_iter()
        .find(|c| c.name == "password")
        .unwrap();
    assert_eq!(
        (change.value.as_deref(), change.secret),
        (Some("rotated-9"), true)
    );
}

#[test]
fn a_raised_cancel_flag_stops_the_chain_before_it_starts() {
    let engine = ScriptEngine::new();
    engine.cancel_flag().cancel();
    let mut ctx = context();
    let script = Script::new("request", "pm.variables.set('ran', '1');");
    let report = engine.run(
        ScriptPhase::PreRequest,
        &[script],
        &mut ctx,
        &Limits::default(),
    );
    assert_eq!(report.error.unwrap().kind, ScriptErrorKind::Cancelled);
    assert!(ctx.changes().is_empty());
}

#[path = "prelude_tests.rs"]
mod prelude_tests;
