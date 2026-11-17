//! Escape suite for the script sandbox (plan 05 §4.3). Limits and redaction: `script_limits.rs`.
//!
//! Each probe is a script a hostile collection could carry. The assertions pin what the
//! probe returned, so this file is also the record of how the sandbox answers each one.
//! The rule being checked: a probe is either unavailable or harmless — it never reaches
//! the host, never outlives its chain, never takes the app down.

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

// ---------------------------------------------------------------- what exists

#[test]
fn the_global_object_holds_only_ecmascript_and_the_documented_extras() {
    let names = lines("console.log(Object.getOwnPropertyNames(globalThis).sort().join(' '))");
    assert_eq!(
        names,
        vec![
            "AggregateError Array ArrayBuffer AsyncDisposableStack Atomics BigInt BigInt64Array \
             BigUint64Array Boolean DataView Date DisposableStack Error EvalError Float16Array \
             Float32Array Float64Array Function Infinity Int16Array Int32Array Int8Array \
             InternalError Iterator JSON Map Math NaN Number Object Promise Proxy RangeError \
             ReferenceError Reflect RegExp Set SharedArrayBuffer String SuppressedError Symbol \
             SyntaxError TypeError URIError \
             Uint16Array Uint32Array Uint8Array Uint8ClampedArray WeakMap WeakSet atob btoa \
             console decodeURI decodeURIComponent encodeURI encodeURIComponent escape eval \
             globalThis isFinite isNaN parseFloat parseInt pm queueMicrotask undefined unescape"
        ]
    );
}

#[test]
fn nothing_that_reaches_the_machine_is_defined() {
    let probes = [
        "require",
        "process",
        "module",
        "exports",
        "std",
        "os",
        "bjson",
        "scriptArgs",
        "print",
        "fetch",
        "XMLHttpRequest",
        "WebSocket",
        "setTimeout",
        "setInterval",
        "setImmediate",
        "performance",
        "navigator",
        "window",
        "self",
        "global",
        "Deno",
        "Bun",
        "__dirname",
        "WeakRef",
        "Worker",
        "__papan_host",
        "host",
        "call",
    ];
    let source = format!(
        "console.log({}.map((n) => typeof globalThis[n] + ':' + eval('typeof ' + n)).join(' '))",
        serde_json::to_string(&probes).unwrap()
    );
    let expected = vec!["undefined:undefined"; probes.len()].join(" ");
    assert_eq!(lines(&source), vec![expected]);
}

#[test]
fn atomics_cannot_park_the_script_thread() {
    // `Atomics.wait` would sleep where the interrupt handler cannot see it.
    assert_eq!(
        lines(
            "const cell = new Int32Array(new SharedArrayBuffer(4));
             try { console.log(Atomics.wait(cell, 0, 0)); } catch (e) { console.log(e.name + ': ' + e.message); }
             try { console.log(Atomics.wait(cell, 0, 0, 60000)); } catch (e) { console.log(e.name + ': ' + e.message); }"
        ),
        vec!["TypeError: cannot block in this thread"; 2]
    );
}

// ---------------------------------------------------------------- string to code

#[test]
fn eval_and_function_work_but_see_the_same_empty_world() {
    // Left enabled on purpose (plan §4): they reach nothing the script could not already.
    assert_eq!(
        lines(
            "console.log(eval('1 + 1'), new Function('return typeof require')(),
               (0, eval)('typeof process'), Function('return this')() === globalThis);"
        ),
        vec!["2 undefined undefined true"]
    );
}

#[test]
fn constructor_constructor_returns_the_sandbox_global_and_nothing_more() {
    assert_eq!(
        lines(
            "const viaPm = pm.constructor.constructor('return this')();
             const viaShim = pm.environment.get.constructor('return this')();
             const viaConsole = console.log.constructor.constructor('return typeof require')();
             const viaError = (() => { try { null.x } catch (e) { return e.constructor.constructor('return this')(); } })();
             console.log(viaPm === globalThis, viaShim === globalThis, viaConsole, viaError === globalThis,
               typeof viaPm.process, typeof viaPm.require);"
        ),
        vec!["true true undefined true undefined undefined"]
    );
}

#[test]
fn import_is_unavailable() {
    // Static import: scripts are not modules.
    let report = run("import fs from 'fs';");
    let error = report.error.unwrap();
    assert_eq!(error.kind, ScriptErrorKind::Exception);
    assert!(
        error.message.starts_with("SyntaxError"),
        "{}",
        error.message
    );

    // Dynamic import: there is no module loader, so the promise rejects.
    let mut ctx = context();
    run_in(
        &mut ctx,
        "try { import('os').then(() => pm.variables.set('r', 'resolved'), (e) => pm.variables.set('r', 'rejected ' + e.name)); }
         catch (e) { pm.variables.set('r', 'threw ' + e.name); }",
        &Limits::default(),
    );
    assert_eq!(ctx.layers.runtime.values["r"], "rejected ReferenceError");
}

// ---------------------------------------------------------------- walking the host objects

#[test]
fn host_objects_are_plain_frozen_javascript_with_no_native_handles() {
    assert_eq!(
        lines(
            "const proto = Object.getPrototypeOf;
             console.log(
               Object.isFrozen(pm), Object.isFrozen(pm.environment), Object.isFrozen(pm.request),
               proto(pm) === Object.prototype, proto(pm.environment.get) === Function.prototype,
               pm.__proto__ === Object.prototype,
               Object.getOwnPropertyNames(pm).sort().join(','));
             console.log(Object.getOwnPropertySymbols(pm).length, Reflect.ownKeys(pm.environment).join(','));"
        ),
        vec![
            "true true true true true true \
             collectionVariables,environment,expect,globals,request,response,sendRequest,test,variables",
            "0 get,has,set,unset,toObject",
        ]
    );
}

#[test]
fn the_native_function_cannot_be_reached_from_a_shim() {
    // The prelude is strict-mode code holding the native function in a closure.
    assert_eq!(
        lines(
            "const out = [];
             const attempt = (label, fn) => { try { out.push(label + '=' + fn()); } catch (e) { out.push(label + '!' + e.name); } };
             attempt('caller', () => pm.environment.get.caller);
             attempt('arguments', () => pm.environment.get.arguments);
             attempt('toString', () => pm.environment.get.toString().includes('[native code]'));
             attempt('callee', () => { let seen; const spy = { get length() { seen = arguments.callee.caller; return 0; } }; pm.environment.get(spy); return seen; });
             attempt('stack', () => { try { pm.environment.set(1); } catch (e) { return /native/.test(e.stack); } });
             console.log(out.join(' '));"
        ),
        vec!["caller!TypeError arguments!TypeError toString=false callee!TypeError stack=false"]
    );
}

#[test]
fn pm_cannot_be_replaced_extended_or_polluted() {
    assert_eq!(
        lines(
            "const original = pm.environment.get;
             pm.environment.get = () => 'hijacked';
             pm.extra = 1;
             pm.environment = {};
             delete pm.test;
             const deleted = delete globalThis.pm;
             globalThis.pm = 'gone';
             let strict;
             try { (function () { 'use strict'; pm.environment.get = 1; })(); } catch (e) { strict = e.name; }
             let redefine;
             try { Object.defineProperty(globalThis, 'pm', { value: 1 }); } catch (e) { redefine = e.name; }
             console.log(pm.environment.get === original, pm.extra, typeof pm.environment.set,
               typeof pm.test, deleted, typeof pm, strict, redefine);"
        ),
        vec!["true undefined function function false object TypeError TypeError"]
    );
}

#[test]
fn prototype_pollution_stays_inside_the_script_and_dies_with_the_chain() {
    let mut ctx = context();
    let polluted = run_in(
        &mut ctx,
        "Object.prototype.polluted = 'yes';
         Object.prototype.secret = true; Object.prototype.scope = 'globals';
         Array.prototype.extra = 1;
         globalThis.leak = 'left behind';
         pm.environment.set('after', 'pollution');
         console.log(pm.environment.get('after'), JSON.stringify(pm.environment.toObject()), pm.environment.polluted);",
        &Limits::default(),
    );
    assert_eq!(polluted.error, None);
    // The host kept its own view: the value went to the environment as a non-secret.
    assert_eq!(
        polluted.console[0].text,
        r#"pollution {"after":"pollution","host":"api.test","token":"[REDACTED]"} yes"#
    );
    let change = &ctx.changes()[0];
    assert_eq!((change.name.as_str(), change.secret), ("after", false));
    assert!(ctx.layers.globals.values.is_empty());

    // A new chain is a new runtime: nothing of the first one is left.
    let fresh = run_in(
        &mut ctx,
        "console.log(typeof leak, ({}).polluted, [].extra, pm.environment.get('after'))",
        &Limits::default(),
    );
    assert_eq!(
        fresh.console[0].text,
        "undefined undefined undefined pollution"
    );
}

#[test]
fn breaking_the_builtins_only_breaks_the_scripts_own_pm_calls() {
    // The prelude uses JSON and Array methods; a script that sabotages them hurts itself.
    let mut ctx = context();
    let report = run_in(
        &mut ctx,
        r#"JSON.stringify = () => '{"method":"DELETE","url":"https://evil.test/","headers":[["X","a\\r\\nInjected: 1"]]}';
           try { pm.request.url = 'https://x.test/'; } catch (e) { console.log(e.name + ': ' + e.message); }
           JSON.stringify = () => 'not json';
           try { pm.request.method = 'PUT'; } catch (e) { console.log(e.message); }
           JSON.parse = () => ({ method: 1, url: {}, headers: 'nope' });
           try { pm.request.headers.add({ key: 'A', value: 'b' }); } catch (e) { console.log(e.name); }"#,
        &Limits::default(),
    );
    assert_eq!(report.error, None);
    let seen: Vec<_> = report.console.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(
        seen,
        vec![
            "TypeError: Header `X` cannot contain a line break.",
            "The request has an invalid shape.",
            "TypeError",
        ]
    );
    assert_eq!(ctx.request, context().request);
}

// ---------------------------------------------------------------- Proxy and Symbol tricks

#[test]
fn proxies_symbols_and_coercion_traps_get_no_special_treatment() {
    assert_eq!(
        lines(
            "const out = [];
             const attempt = (label, fn) => { try { out.push(label + '=' + fn()); } catch (e) { out.push(label + '!' + e.name + ':' + e.message); } };
             let coerced = 0;
             const sneaky = { toString() { coerced++; return 'token'; }, [Symbol.toPrimitive]() { coerced++; return 'token'; } };
             attempt('objectName', () => pm.environment.get(sneaky));
             attempt('symbolName', () => pm.environment.get(Symbol('token')));
             attempt('proxyName', () => pm.environment.get(new Proxy({}, { get: () => () => 'token' })));
             attempt('stringObject', () => pm.environment.get(new String('token')));
             attempt('coerced', () => coerced);
             attempt('symbolValue', () => { pm.variables.set('s', Symbol('v')); return pm.variables.get('s'); });
             attempt('proxyValue', () => { pm.variables.set('p', new Proxy({ a: 1 }, {})); return pm.variables.get('p'); });
             attempt('throwingValue', () => { pm.variables.set('t', { toJSON() { throw new Error('trap'); } }); return pm.variables.get('t'); });
             attempt('proxyHeader', () => pm.request.headers.add(new Proxy({}, { get: (_, k) => (k === 'key' ? 'X\\r\\nEvil' : 'v') })));
             attempt('proxiedPm', () => new Proxy(pm, { get: (t, k) => (k === 'secret' ? 1 : t[k]) }).environment.get('host'));
             console.log(out.join(' | '));"
        ),
        vec![
            "objectName!TypeError:A variable name must be a string. | \
             symbolName!TypeError:A variable name must be a string. | \
             proxyName!TypeError:A variable name must be a string. | \
             stringObject!TypeError:A variable name must be a string. | \
             coerced=0 | symbolValue=Symbol(v) | proxyValue={\"a\":1} | \
             throwingValue=[object Object] | \
             proxyHeader!TypeError:A header name is not a valid HTTP header name. | \
             proxiedPm=api.test"
        ]
    );
}

#[test]
fn text_that_is_not_valid_unicode_is_refused_by_the_host() {
    assert_eq!(
        lines(
            "try { pm.variables.set('lone', '\\uD800'); console.log('stored'); }
             catch (e) { console.log(e.name + ': ' + e.message); }
             console.log(pm.variables.has('lone'));"
        ),
        vec!["TypeError: A string is not valid Unicode text.", "false"]
    );
}
