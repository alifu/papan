//! The QuickJS sandbox: one fresh runtime and context per script chain.
//!
//! What a script can reach is ECMAScript built-ins plus `pm`, `console`, `atob` and `btoa`.
//! There is no module loader, no `std`/`os` module, no `require`, no `fetch`, no timers.
//! `eval` and `Function` are left in place on purpose: they only reach what is listed here.
//! The single native entry point is the host function of [`super::pm`].
//!
//! Limits: a memory cap, a stack cap, and an interrupt handler that stops the chain at its
//! wall-clock deadline or when the cancel flag is raised. The interrupt is not catchable
//! from JavaScript.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rquickjs::context::{intrinsic, EvalOptions};
use rquickjs::function::Rest;
use rquickjs::{Context, Ctx, Error, Exception, Function, Runtime, Value};

use super::chain::Script;
use super::pm::HostState;
use super::report::{ScriptError, ScriptErrorKind};

/// The file name user scripts are evaluated under; stack traces are searched for it.
const SCRIPT_FILE: &str = "script";
const PRELUDE_FILE: &str = "prelude";
/// A host call takes an operation name and at most three string arguments.
const MAX_HOST_ARGS: usize = 4;

const PRELUDE: &str = concat!(
    "(function (host) {\n'use strict';\n",
    include_str!("expect.js"),
    "\n",
    include_str!("prelude.js"),
    "\n})"
);

/// Resource limits of one script chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Heap the JavaScript engine may allocate.
    pub memory_bytes: usize,
    /// Native stack the JavaScript engine may use.
    pub stack_bytes: usize,
    /// Wall-clock time for the whole chain, `pm.sendRequest` calls included.
    pub timeout: Duration,
    /// `pm.sendRequest` calls allowed per chain.
    pub max_send_requests: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            memory_bytes: 32 * 1024 * 1024,
            stack_bytes: 512 * 1024,
            timeout: Duration::from_secs(5),
            max_send_requests: 5,
        }
    }
}

/// Raised from any thread to stop a running chain (`cancel_request`). Clones share it.
#[derive(Debug, Clone, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// Whether `other` is a clone of this flag.
    pub fn is_same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Why the interrupt handler stopped the chain, if it did.
type Stop = Rc<Cell<Option<ScriptErrorKind>>>;

/// Runs `scripts` in order in one fresh sandbox. Returns the host state as the scripts
/// left it, and the error that ended the chain early, if any.
pub(super) fn run_chain(
    host: HostState,
    scripts: &[Script],
    limits: &Limits,
    cancel: &CancelFlag,
) -> (HostState, Option<ScriptError>) {
    let deadline = host.deadline;
    let state = Rc::new(RefCell::new(host));
    let error = execute(&state, scripts, limits, cancel, deadline).err();
    // The runtime is gone, so this is the last reference; the clone is a safe fallback.
    let host = Rc::try_unwrap(state)
        .map(RefCell::into_inner)
        .unwrap_or_else(|shared| shared.borrow().clone());
    (host, error)
}

fn execute(
    state: &Rc<RefCell<HostState>>,
    scripts: &[Script],
    limits: &Limits,
    cancel: &CancelFlag,
    deadline: Instant,
) -> Result<(), ScriptError> {
    let stop: Stop = Rc::new(Cell::new(None));
    let runtime = Runtime::new().map_err(|e| internal(&e))?;
    runtime.set_memory_limit(limits.memory_bytes);
    runtime.set_max_stack_size(limits.stack_bytes);
    runtime.set_interrupt_handler(Some(Box::new({
        let (stop, cancel) = (Rc::clone(&stop), cancel.clone());
        move || should_stop(&stop, &cancel, deadline)
    })));
    let context = Context::builder()
        .with::<intrinsic::Date>()
        .with::<intrinsic::Eval>()
        .with::<intrinsic::RegExpCompiler>()
        .with::<intrinsic::RegExp>()
        .with::<intrinsic::Json>()
        .with::<intrinsic::Proxy>()
        .with::<intrinsic::MapSet>()
        .with::<intrinsic::TypedArrays>()
        .with::<intrinsic::Promise>()
        .build(&runtime)
        .map_err(|e| internal(&e))?;

    context.with(|ctx| {
        install_prelude(&ctx, Rc::clone(state))
            .map_err(|e| classify(&ctx, e, &stop, None, ScriptErrorKind::Internal))?;
        for script in scripts {
            let wrapped = format!("(function () {{{}\n}})();", script.source);
            let result = eval(&ctx, wrapped, SCRIPT_FILE)
                .map(|_: Value| run_jobs(&ctx, &stop, cancel, deadline));
            let stopped = stop.get().is_some();
            match result {
                Ok(()) if !stopped => {}
                Ok(()) => return Err(stop_error(&stop, &script.label)),
                Err(e) => {
                    let label = Some(script.label.as_str());
                    return Err(classify(&ctx, e, &stop, label, ScriptErrorKind::Exception));
                }
            }
        }
        Ok(())
    })
}

fn should_stop(stop: &Stop, cancel: &CancelFlag, deadline: Instant) -> bool {
    if stop.get().is_some() {
        return true;
    }
    let reason = if cancel.is_cancelled() {
        Some(ScriptErrorKind::Cancelled)
    } else if Instant::now() >= deadline {
        Some(ScriptErrorKind::Timeout)
    } else {
        None
    };
    stop.set(reason);
    reason.is_some()
}

/// Runs promise reactions queued by the script, within the same limits.
fn run_jobs(ctx: &Ctx<'_>, stop: &Stop, cancel: &CancelFlag, deadline: Instant) {
    while !should_stop(stop, cancel, deadline) && ctx.execute_pending_job() {}
}

fn eval<'js>(
    ctx: &Ctx<'js>,
    source: impl Into<Vec<u8>>,
    file: &str,
) -> rquickjs::Result<Value<'js>> {
    let mut options = EvalOptions::default();
    options.strict = false;
    options.filename = Some(file.to_string());
    ctx.eval_with_options(source, options)
}

fn install_prelude<'js>(ctx: &Ctx<'js>, state: Rc<RefCell<HostState>>) -> rquickjs::Result<()> {
    let host = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, args: Rest<Value<'js>>| -> rquickjs::Result<Option<String>> {
            host_call(&state, &args.0).map_err(|message| Exception::throw_type(&ctx, &message))
        },
    )?;
    let setup: Function = eval(ctx, PRELUDE, PRELUDE_FILE)?
        .into_function()
        .ok_or(Error::Unknown)?;
    setup.call::<_, ()>((host,))
}

/// The one native entry point. Arguments must be primitive strings: nothing is coerced,
/// so no script code runs while the host state is borrowed.
fn host_call(state: &RefCell<HostState>, args: &[Value<'_>]) -> Result<Option<String>, String> {
    if args.is_empty() || args.len() > MAX_HOST_ARGS {
        return Err("Invalid host call.".into());
    }
    let strings = args
        .iter()
        .map(|value| {
            value
                .as_string()
                .ok_or("Host arguments must be strings.")?
                .to_string()
                .map_err(|_| "A string is not valid Unicode text.")
        })
        .collect::<Result<Vec<String>, &str>>()?;
    let mut host = state
        .try_borrow_mut()
        .map_err(|_| "The host is busy.".to_string())?;
    host.dispatch(&strings[0], &strings[1..])
}

fn internal(error: &Error) -> ScriptError {
    let kind = match error {
        Error::Allocation => ScriptErrorKind::Memory,
        _ => ScriptErrorKind::Internal,
    };
    ScriptError {
        kind,
        message: match kind {
            ScriptErrorKind::Memory => memory_message(),
            _ => format!("The script engine could not start: {error}"),
        },
        script: None,
        line: None,
    }
}

fn memory_message() -> String {
    "The script used more memory than allowed.".into()
}

/// What the user reads when a chain was stopped for `kind`.
pub(super) fn stop_message(kind: ScriptErrorKind) -> &'static str {
    match kind {
        ScriptErrorKind::Cancelled => "The script was cancelled.",
        _ => "The script ran longer than allowed and was stopped.",
    }
}

fn stop_error(stop: &Stop, label: &str) -> ScriptError {
    let kind = stop.get().unwrap_or(ScriptErrorKind::Internal);
    ScriptError {
        kind,
        message: stop_message(kind).into(),
        script: Some(label.to_string()),
        line: None,
    }
}

/// Turns an engine error into a [`ScriptError`]. `fallback` is the kind of an ordinary
/// exception: `Exception` for user scripts, `Internal` for the prelude.
fn classify(
    ctx: &Ctx<'_>,
    error: Error,
    stop: &Stop,
    label: Option<&str>,
    fallback: ScriptErrorKind,
) -> ScriptError {
    let script = label.map(str::to_string);
    if stop.get().is_some() {
        // Drop the pending "interrupted" exception.
        let _ = ctx.catch();
        return ScriptError {
            script,
            ..stop_error(stop, "")
        };
    }
    let (kind, message, line) = match error {
        Error::Allocation => (ScriptErrorKind::Memory, memory_message(), None),
        Error::Exception => describe_thrown(&ctx.catch(), fallback),
        other => (ScriptErrorKind::Internal, other.to_string(), None),
    };
    ScriptError {
        kind,
        message,
        script,
        line,
    }
}

fn describe_thrown(
    thrown: &Value<'_>,
    fallback: ScriptErrorKind,
) -> (ScriptErrorKind, String, Option<u32>) {
    let Some(exception) = thrown.as_object().cloned().and_then(Exception::from_object) else {
        let message = thrown
            .as_string()
            .and_then(|text| text.to_string().ok())
            .or_else(|| {
                let json = thrown.ctx().json_stringify(thrown.clone()).ok().flatten()?;
                json.to_string().ok()
            })
            .unwrap_or_else(|| "The script threw a value that cannot be shown.".into());
        return (fallback, format!("Uncaught {message}"), None);
    };
    let name: String = exception
        .as_object()
        .get::<_, Option<String>>("name")
        .ok()
        .flatten()
        .unwrap_or_else(|| "Error".into());
    let text = exception.message().unwrap_or_default();
    let line = exception.stack().as_deref().and_then(script_line);
    let lowered = text.to_lowercase();
    if name == "InternalError" && lowered.contains("out of memory") {
        return (ScriptErrorKind::Memory, memory_message(), line);
    }
    if lowered.contains("stack overflow") || lowered.contains("call stack size exceeded") {
        let message = "The script recursed too deeply.".to_string();
        return (ScriptErrorKind::StackOverflow, message, line);
    }
    (fallback, format!("{name}: {text}"), line)
}

/// The first `script:<line>` position in a stack trace.
fn script_line(stack: &str) -> Option<u32> {
    let marker = format!("{SCRIPT_FILE}:");
    stack.match_indices(&marker).find_map(|(at, _)| {
        let digits: String = stack[at + marker.len()..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse().ok()
    })
}

#[cfg(test)]
#[path = "sandbox_tests.rs"]
mod tests;
