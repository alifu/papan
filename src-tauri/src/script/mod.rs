//! QuickJS sandbox exposing a Postman-like `pm` API (plan 05). Plain Rust, no Tauri types.
//!
//! Entry point: [`ScriptEngine::run`]. It blocks until the chain ends, so call it from
//! `spawn_blocking`. The pipeline builds a [`ScriptContext`], runs the pre-request chain,
//! reads back `context.request` and `context.layers.runtime`, sends, sets
//! `context.response`, runs the post-response chain, then persists `context.changes()`.

pub mod chain;
pub mod context;
pub mod pm;
pub mod report;
pub mod sandbox;
pub mod watchdog;

pub use chain::{Script, ScriptEngine, MAX_SCRIPT_BYTES};
pub use context::{
    validate_request, Layers, Scope, ScriptContext, ScriptRequest, ScriptResponse, VarChange,
    VarLayer,
};
pub use pm::SendRequestBridge;
pub use report::{
    ConsoleLevel, ConsoleLine, Redactor, ScriptError, ScriptErrorKind, ScriptPhase, ScriptReport,
    ScriptScope, ScriptVarChange, TestResult,
};
pub use sandbox::{CancelFlag, Limits};
pub use watchdog::{StrayThreads, MAX_STRAY_THREADS, WATCHDOG_GRACE};
