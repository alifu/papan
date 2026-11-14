//! The JavaScript side: `pm.response`, `pm.test`, `pm.expect`, `console`, `pm.sendRequest`.

use super::*;

#[test]
fn the_response_is_readable_after_the_send_and_the_request_is_read_only() {
    let report = post(
        "const r = pm.response;
         console.log(r.code, r.status, r.responseTime, r.headers.get('content-type'),
           r.json().token, r.text().length, r.headers.has('nope'));
         console.log(pm.request.method);
         try { pm.request.method = 'PUT'; } catch (e) { console.log(e.message); }",
    );
    assert_eq!(
        logged(&report),
        vec![
            "201 Created 42 application/json abc 29 false",
            "GET",
            "pm.request is read-only once the request has been sent.",
        ]
    );
    let none = pre("console.log(pm.response === undefined)", &mut context());
    assert_eq!(logged(&none), vec!["true"]);
}

#[test]
fn tests_are_collected_with_their_failures() {
    let report = post(
        "pm.test('status', () => pm.response.to.have.status(201));
         pm.test('status text', () => pm.response.to.have.status('Created'));
         pm.test('ok', () => { pm.response.to.be.ok; pm.response.to.be.success; });
         pm.test('header', () => pm.response.to.have.header('content-type', 'application/json'));
         pm.test('negated', () => pm.response.to.not.have.status(500));
         pm.test('wrong status', () => pm.response.to.have.status(200));
         pm.test('not an error', () => pm.response.to.be.error);
         pm.test('throws', () => { throw new RangeError('nope'); });
         pm.test('async', async () => {});
         pm.test('expect', () => pm.expect(pm.response.json().items).to.have.lengthOf(3));",
    );
    assert_eq!(report.error, None);
    let results: Vec<_> = report
        .tests
        .iter()
        .map(|t| (t.name.as_str(), t.passed, t.error.as_deref()))
        .collect();
    assert_eq!(
        results,
        vec![
            ("status", true, None),
            ("status text", true, None),
            ("ok", true, None),
            ("header", true, None),
            ("negated", true, None),
            (
                "wrong status",
                false,
                Some("expected response to have status 200 (got 201 Created)")
            ),
            (
                "not an error",
                false,
                Some("expected response to be error (status 201)")
            ),
            ("throws", false, Some("RangeError: nope")),
            (
                "async",
                false,
                Some("Error: asynchronous tests are not supported")
            ),
            (
                "expect",
                false,
                Some("expected [1,2] to have a length of 3")
            ),
        ]
    );
}

/// Each expression must pass as written and fail once negated at the marked spot.
#[test]
fn chai_matchers_pass_and_fail() {
    let passing = [
        "expect(1).to.equal(1)",
        "expect('a').to.not.equal('b')",
        "expect({a: [1, {b: 2}]}).to.eql({a: [1, {b: 2}]})",
        "expect({a: 1}).to.deep.equal({a: 1})",
        "expect({a: 1}).to.not.equal({a: 1})",
        "expect('hello').to.include('ell')",
        "expect([1, 2]).to.include(2)",
        "expect([{a: 1}]).to.deep.include({a: 1})",
        "expect({a: 1, b: 2}).to.include({a: 1})",
        "expect({a: {b: 1}}).to.have.property('a').that.has.property('b', 1)",
        "expect({a: 1}).to.not.have.property('z')",
        "expect('s').to.be.a('string')",
        "expect([]).to.be.an('array')",
        "expect(null).to.be.a('null')",
        "expect(new Date()).to.be.a('date')",
        "expect(5).to.be.above(4).and.below(6)",
        "expect(5).to.be.at.least(5).and.at.most(5)",
        "expect(5).to.be.within(1, 9)",
        "expect(0).to.exist",
        "expect(undefined).to.not.exist",
        "expect(1).to.be.ok",
        "expect(true).to.be.true",
        "expect(false).to.be.false",
        "expect(null).to.be.null",
        "expect(undefined).to.be.undefined",
        "expect(NaN).to.be.NaN",
        "expect('').to.be.empty",
        "expect({}).to.be.empty",
        "expect([1, 2, 3]).to.have.lengthOf(3)",
        "expect('abc').to.match(/^a/)",
        "expect(2).to.be.oneOf([1, 2])",
        "expect({a: 1, b: 2}).to.have.keys('a', 'b')",
        "expect(() => { throw new TypeError('bad thing'); }).to.throw('bad')",
        "expect(() => { throw new TypeError('bad'); }).to.throw(TypeError)",
        "expect(() => {}).to.not.throw()",
        "expect(1).to.eq(1)",
        "expect([1]).to.contain(1)",
        "expect(3).to.be.gt(2).and.lt(4).and.gte(3).and.lte(3)",
    ];
    for expression in passing {
        let source = format!("const expect = pm.expect; {expression};");
        let report = pre(&source, &mut context());
        assert_eq!(report.error, None, "{expression}");
    }
    let failing = [
        ("expect(1).to.equal(2)", "expected 1 to equal 2"),
        ("expect(1).to.not.equal(1)", "expected 1 not to equal 1"),
        (
            "expect({a: 1}).to.eql({a: 2})",
            r#"expected {"a":1} to deeply equal {"a":2}"#,
        ),
        (
            "expect('x').to.be.a('number')",
            r#"expected "x" to be a number"#,
        ),
        (
            "expect('abc').to.include('z')",
            r#"expected "abc" to include "z""#,
        ),
        (
            "expect({}).to.have.property('a')",
            r#"expected {} to have property "a""#,
        ),
        ("expect(1).to.be.above(2)", "expected 1 to be above 2"),
        ("expect(null).to.exist", "expected null to exist"),
        ("expect(0).to.be.ok", "expected 0 to be truthy"),
        ("expect(1).to.be.true", "expected 1 to be true"),
        ("expect([1]).to.be.empty", "expected [1] to be empty"),
        (
            "expect(() => {}).to.throw()",
            "expected [Function] to throw",
        ),
    ];
    for (expression, message) in failing {
        let source = format!("const expect = pm.expect; {expression};");
        let error = pre(&source, &mut context()).error.expect(expression);
        assert_eq!(error.message, format!("AssertionError: {message}"));
    }
}

#[test]
fn console_formats_values_and_levels() {
    let report = pre(
        "console.log('a', 1, true, null, undefined, {b: [2]}, new Error('e'));
         console.info('i'); console.warn('w'); console.error('e'); console.debug('d');
         const cyclic = {}; cyclic.self = cyclic; console.log(cyclic);",
        &mut context(),
    );
    assert_eq!(
        logged(&report),
        vec![
            r#"a 1 true null undefined {"b":[2]} Error: e"#,
            "i",
            "w",
            "e",
            "d",
            "[object Object]"
        ]
    );
    let levels: Vec<_> = report.console.iter().map(|l| l.level).collect();
    assert_eq!(
        levels[1..4],
        [ConsoleLevel::Info, ConsoleLevel::Warn, ConsoleLevel::Error]
    );
}

struct FakeBridge {
    seen: Mutex<Vec<ScriptRequest>>,
}

impl SendRequestBridge for FakeBridge {
    fn send(&self, request: &ScriptRequest, remaining: Duration) -> Result<ScriptResponse, String> {
        assert!(remaining <= Duration::from_secs(5) && !remaining.is_zero());
        self.seen.lock().unwrap().push(request.clone());
        if request.url.contains("fail") {
            return Err("connection refused".into());
        }
        Ok(ScriptResponse {
            code: 200,
            status: "OK".into(),
            headers: vec![("X-From".into(), "bridge".into())],
            body: r#"{"token":"t-1"}"#.into(),
            response_time_ms: 7,
        })
    }
}

#[test]
fn send_request_goes_through_the_bridge_and_is_limited_per_chain() {
    let bridge = Arc::new(FakeBridge {
        seen: Mutex::new(Vec::new()),
    });
    let engine = ScriptEngine::new().with_bridge(bridge.clone());
    let script = Script::new(
        "request",
        "pm.sendRequest({url: 'https://auth.test/login', method: 'POST',
             header: {'Content-Type': 'application/json'}, body: {mode: 'raw', raw: '{}'}},
           (err, res) => {
             console.log(err, res.code, res.status, res.headers.get('x-from'), res.json().token, res.responseTime);
             pm.environment.set('token', res.json().token);
           });
         pm.sendRequest('https://auth.test/fail', (err, res) => console.log(err.message, res));
         for (let i = 0; i < 3; i++) pm.sendRequest('https://auth.test/' + i, () => {});
         try { pm.sendRequest('https://auth.test/sixth', () => {}); } catch (e) { console.log(e.message); }
         try { pm.sendRequest({url: 'x', method: 'BAD METHOD'}); } catch (e) { console.log(e.message); }",
    );
    let mut ctx = context();
    let report = engine.run(
        ScriptPhase::PreRequest,
        &[script],
        &mut ctx,
        &Limits::default(),
    );
    assert_eq!(
        logged(&report),
        vec![
            "null 200 OK bridge t-1 7",
            "connection refused undefined",
            "pm.sendRequest: too many requests in one script chain.",
            "The request method is not a valid HTTP method.",
        ]
    );
    let seen = bridge.seen.lock().unwrap();
    assert_eq!(seen.len(), 5);
    assert_eq!(
        seen[0],
        ScriptRequest {
            method: "POST".into(),
            url: "https://auth.test/login".into(),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: Some("{}".into()),
        }
    );
    assert_eq!(ctx.layers.environment.unwrap().values["token"], "t-1");
}

#[test]
fn send_request_without_a_bridge_hands_the_callback_an_error() {
    let report = pre(
        "pm.sendRequest('https://a.test', (err) => console.log(err.message));",
        &mut context(),
    );
    assert_eq!(
        logged(&report),
        vec!["pm.sendRequest is not available here."]
    );
}
