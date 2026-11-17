use super::*;
use crate::script::context::{Layers, VarLayer};

fn host(phase: ScriptPhase) -> HostState {
    let ctx = ScriptContext::new(
        Layers {
            environment: Some(VarLayer::default()),
            ..Layers::default()
        },
        ScriptRequest {
            method: "GET".into(),
            url: "http://a.test".into(),
            ..Default::default()
        },
    );
    HostState::new(phase, ctx, None, Instant::now() + Duration::from_secs(5), 5)
}

fn call(host: &mut HostState, op: &str, args: &[&str]) -> Reply {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    host.dispatch(op, &args)
}

#[test]
fn unknown_operations_and_wrong_arity_are_rejected() {
    let mut host = host(ScriptPhase::PreRequest);
    for (op, args) in [
        ("nope", vec![]),
        ("var.get", vec!["environment"]),
        ("var.set", vec!["environment", "a"]),
        ("var.get", vec!["__proto__", "a"]),
        ("req.get", vec!["extra"]),
        ("console", vec!["shout", "x"]),
        ("test", vec!["t", "maybe"]),
    ] {
        assert!(call(&mut host, op, &args).is_err(), "{op} {args:?}");
    }
}

#[test]
fn variables_round_trip_through_the_host() {
    let mut host = host(ScriptPhase::PreRequest);
    assert_eq!(
        call(&mut host, "var.set", &["environment", "k", "v"]),
        Ok(None)
    );
    assert_eq!(
        call(&mut host, "var.get", &["environment", "k"]),
        Ok(Some("v".into()))
    );
    assert_eq!(
        call(&mut host, "var.get", &["any", "k"]),
        Ok(Some("v".into()))
    );
    assert_eq!(call(&mut host, "var.has", &["globals", "k"]), Ok(None));
    assert_eq!(
        call(&mut host, "var.all", &["environment"]),
        Ok(Some(r#"{"k":"v"}"#.into()))
    );
    assert_eq!(
        call(&mut host, "var.unset", &["environment", "k"]),
        Ok(None)
    );
    assert_eq!(call(&mut host, "var.has", &["any", "k"]), Ok(None));
}

#[test]
fn environment_set_without_environment_falls_back_to_runtime_and_warns_once() {
    let mut host = host(ScriptPhase::PreRequest);
    host.ctx.layers.environment = None;
    call(&mut host, "var.set", &["environment", "a", "1"]).unwrap();
    call(&mut host, "var.set", &["environment", "b", "2"]).unwrap();
    assert_eq!(
        call(&mut host, "var.unset", &["environment", "a"]),
        Ok(None)
    );
    assert_eq!(host.ctx.layers.runtime.values.len(), 2);
    assert_eq!(call(&mut host, "var.get", &["environment", "a"]), Ok(None));
    assert_eq!(
        call(&mut host, "var.all", &["environment"]),
        Ok(Some("{}".into()))
    );
    assert_eq!(host.console.len(), 1);
    assert_eq!(host.console[0].level, ConsoleLevel::Warn);
}

#[test]
fn request_changes_are_validated_and_only_allowed_before_the_send() {
    let mut host = host(ScriptPhase::PreRequest);
    let good = r#"{"method":"POST","url":"http://b.test","headers":[["X-A","1"]]}"#;
    assert_eq!(call(&mut host, "req.set", &[good]), Ok(None));
    assert_eq!(call(&mut host, "req.setBody", &["body"]), Ok(None));
    assert_eq!(host.ctx.request.method, "POST");
    assert_eq!(host.ctx.request.body.as_deref(), Some("body"));
    assert_eq!(call(&mut host, "req.body", &[]), Ok(Some("body".into())));
    assert_eq!(call(&mut host, "req.setBody", &[]), Ok(None));
    assert_eq!(host.ctx.request.body, None);

    let before = host.ctx.request.clone();
    for bad in [
        "not json",
        r#"{"method":"POST","url":"u"}"#,
        r#"{"method":"POST","url":"u","headers":[],"extra":1}"#,
        r#"{"method":"P T","url":"u","headers":[]}"#,
        r#"{"method":"GET","url":"u","headers":[["A","x\r\ny"]]}"#,
        r#"{"method":"GET","url":"u","headers":[["A",1]]}"#,
    ] {
        assert!(call(&mut host, "req.set", &[bad]).is_err(), "{bad}");
    }
    assert_eq!(host.ctx.request, before);

    let mut post = self::host(ScriptPhase::PostResponse);
    assert!(call(&mut post, "req.set", &[good]).is_err());
    assert!(call(&mut post, "req.setBody", &["x"]).is_err());
    assert!(call(&mut post, "req.get", &[]).is_ok());
}

#[test]
fn console_output_is_capped_in_lines_and_line_length() {
    let mut host = host(ScriptPhase::PreRequest);
    let long = "é".repeat(RAW_TEXT_BYTES);
    for _ in 0..MAX_CONSOLE_LINES + 10 {
        call(&mut host, "console", &["log", &long]).unwrap();
    }
    assert_eq!(host.console.len(), MAX_CONSOLE_LINES);
    assert!(host.console_truncated);
    assert!(host.console[0].text.len() <= RAW_TEXT_BYTES + '…'.len_utf8());
    assert!(host.console[0].text.ends_with('…'));
}

#[test]
fn tests_are_capped_and_a_passing_test_has_no_error() {
    let mut host = host(ScriptPhase::PostResponse);
    call(&mut host, "test", &["ok", "1", "ignored"]).unwrap();
    call(&mut host, "test", &["bad", "0", "why"]).unwrap();
    assert_eq!(host.tests[0].error, None);
    assert_eq!(host.tests[1].error.as_deref(), Some("why"));
    for _ in 2..MAX_TESTS {
        call(&mut host, "test", &["t", "1"]).unwrap();
    }
    assert!(call(&mut host, "test", &["t", "1"]).is_err());
    assert_eq!(host.tests.len(), MAX_TESTS);
}

#[test]
fn base64_follows_browser_rules() {
    let mut host = host(ScriptPhase::PreRequest);
    assert_eq!(
        call(&mut host, "btoa", &["user:päss"]),
        Ok(Some("dXNlcjpw5HNz".into()))
    );
    assert_eq!(
        call(&mut host, "atob", &["dXNlcjpw5HNz"]),
        Ok(Some("user:päss".into()))
    );
    assert_eq!(call(&mut host, "atob", &[" aGk \n"]), Ok(Some("hi".into())));
    assert!(call(&mut host, "btoa", &["snow ☃"]).is_err());
    assert!(call(&mut host, "atob", &["***"]).is_err());
}

#[test]
fn send_without_a_bridge_reports_an_error_to_the_script() {
    let mut host = host(ScriptPhase::PreRequest);
    let spec = r#"{"method":"GET","url":"http://a.test","headers":[],"body":null}"#;
    let reply = call(&mut host, "send", &[spec]).unwrap().unwrap();
    assert!(reply.contains(r#""ok":false"#), "{reply}");
    assert!(call(&mut host, "send", &[r#"{"method":"GET"}"#]).is_err());
}
