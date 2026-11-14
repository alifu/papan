//! Runs one script chain off the async runtime and serves its `pm.sendRequest` calls
//! through [`HttpEngine`] (plan 05 §4.2).
//!
//! The chain blocks a `spawn_blocking` worker. `pm.sendRequest` cannot reach the async
//! engine from there, so the bridge posts each call on a channel that this task serves
//! while it waits for the chain; the script thread blocks on the reply, bounded by the
//! time left in the chain. Cancel (`cancel_request`) raises the chain's flag, which stops
//! the JavaScript and any call being served.

use std::path::Path;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

use super::resolve::Resolved;
use super::script_map::{content_of, script_response, with_secrets};
use super::HttpEngine;
use crate::script::{
    CancelFlag, Limits, Redactor, Script, ScriptContext, ScriptEngine, ScriptErrorKind,
    ScriptPhase, ScriptReport, ScriptRequest, ScriptResponse, SendRequestBridge, StrayThreads,
};
use crate::store::types::{RequestContent, RequestLocation};

/// Largest response body a script reads (plan 05 §4.2).
pub const SCRIPT_BODY_BYTES: u64 = 5 * 1024 * 1024;
const CANCEL_POLL: Duration = Duration::from_millis(20);

type Reply = Result<ScriptResponse, String>;

struct Call {
    request: ScriptRequest,
    remaining: Duration,
    reply: mpsc::SyncSender<Reply>,
}

struct ChannelBridge {
    calls: UnboundedSender<Call>,
}

impl SendRequestBridge for ChannelBridge {
    fn send(&self, request: &ScriptRequest, remaining: Duration) -> Reply {
        let (reply, answer) = mpsc::sync_channel(1);
        let call = Call {
            request: request.clone(),
            remaining,
            reply,
        };
        self.calls
            .send(call)
            .map_err(|_| "pm.sendRequest is not available.".to_string())?;
        answer
            .recv_timeout(remaining)
            .map_err(|_| "pm.sendRequest timed out.".to_string())?
    }
}

/// What a chain needs from the send it belongs to.
pub struct ChainRunner<'a> {
    pub engine: &'a HttpEngine,
    pub location: &'a RequestLocation,
    /// The draft: `pm.sendRequest` uses its settings (TLS, redirects, timeout).
    pub draft: &'a RequestContent,
    pub cancel: CancelFlag,
    pub redactor: Redactor,
    /// Secret values of the send's scopes: masked in whatever `pm.sendRequest` shows or logs.
    pub secrets: Vec<String>,
    pub strays: StrayThreads,
    pub limits: Limits,
}

impl ChainRunner<'_> {
    /// Runs `scripts` against `context` and returns the context after the chain.
    pub async fn run(
        &self,
        phase: ScriptPhase,
        scripts: Vec<Script>,
        context: ScriptContext,
    ) -> (ScriptContext, ScriptReport) {
        let (calls, mut pending) = unbounded_channel();
        let engine = ScriptEngine::new()
            .with_bridge(Arc::new(ChannelBridge { calls }))
            .with_cancel(self.cancel.clone())
            .with_redactor(Arc::clone(&self.redactor))
            .with_strays(self.strays.clone());
        let limits = self.limits.clone();
        let fallback = context.clone();
        let mut chain = tokio::task::spawn_blocking(move || {
            let mut context = context;
            let report = engine.run(phase, &scripts, &mut context, &limits);
            (context, report)
        });
        loop {
            tokio::select! {
                joined = &mut chain => {
                    return joined.unwrap_or_else(|error| {
                        tracing::error!(%error, "the script task failed");
                        let report = ScriptReport::failed(
                            phase,
                            ScriptErrorKind::Internal,
                            "The script engine failed unexpectedly.",
                        );
                        (fallback, report)
                    });
                }
                Some(call) = pending.recv() => {
                    let reply = self.serve(&call.request, call.remaining).await;
                    // The script may have stopped waiting; nothing to do then.
                    let _ = call.reply.send(reply);
                }
            }
        }
    }

    /// One `pm.sendRequest`: the same engine path as a send, under its own id. The
    /// response is not kept, so it takes no slot of the user's response cache.
    async fn serve(&self, request: &ScriptRequest, remaining: Duration) -> Reply {
        let id = uuid::Uuid::new_v4().to_string();
        let content = content_of(request, self.draft);
        let resolved = with_secrets(Resolved::unchanged(content), &self.secrets);
        let body_file = self.engine.unkept_body(&id).ok();
        let send = async {
            let meta = self
                .engine
                .send_unkept(&id, self.location, &resolved)
                .await
                .map_err(|error| (self.redactor)(&error.message))?;
            let body = match &body_file {
                Some(path) => read_capped(path).await,
                None => Vec::new(),
            };
            Ok(script_response(&meta, &body))
        };
        let reply = tokio::select! {
            outcome = tokio::time::timeout(remaining, send) => {
                outcome.unwrap_or_else(|_| Err("pm.sendRequest timed out.".to_string()))
            }
            () = cancelled(&self.cancel) => Err("The request was cancelled.".to_string()),
        };
        if let Some(path) = body_file {
            // Missing when the send failed before a body arrived.
            let _ = tokio::fs::remove_file(path).await;
        }
        reply
    }
}

/// Resolves once `flag` is raised.
pub async fn cancelled(flag: &CancelFlag) {
    while !flag.is_cancelled() {
        tokio::time::sleep(CANCEL_POLL).await;
    }
}

/// The first [`SCRIPT_BODY_BYTES`] of a response body file; empty if it cannot be read.
pub async fn read_capped(path: &Path) -> Vec<u8> {
    use tokio::io::AsyncReadExt as _;
    let read = async {
        let file = tokio::fs::File::open(path).await.ok()?;
        let mut bytes = Vec::new();
        file.take(SCRIPT_BODY_BYTES)
            .read_to_end(&mut bytes)
            .await
            .ok()?;
        Some(bytes)
    };
    read.await.unwrap_or_else(|| {
        tracing::warn!("could not read the response body for a script");
        Vec::new()
    })
}
