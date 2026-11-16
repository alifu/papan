//! The real [`Host`]: requests go through the HTTP pipeline (scripts, variables, cookies,
//! history) and scripts through the QuickJS sandbox (plan 13 §4).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use super::check::Sent;
use super::runner::{Host, ScriptDone};
use super::types::RunEvent;
use crate::engine::http::mask::mask_secrets;
use crate::engine::http::script_bridge::{read_capped, ChainRunner};
use crate::engine::http::scripted::{run_send_with, SendDeps};
use crate::script::{
    Layers, Limits, Redactor, Script, ScriptContext, ScriptPhase, ScriptRequest, ScriptResponse,
};
use crate::store::types::{RequestContent, RequestKind, RequestLocation};

/// Sends the events of a run somewhere (the webview).
pub type Emit = Box<dyn Fn(RunEvent) + Send + Sync>;

pub struct EngineHost<'a> {
    pub deps: SendDeps<'a>,
    pub collection_id: String,
    /// Raised by `cancel_journey`.
    pub cancel: Arc<AtomicBool>,
    /// Secret values of the collection's variables: hidden in everything an event carries.
    pub secrets: Vec<String>,
    /// Scripts of a cloned collection do not run until the user trusts it.
    pub scripts_trusted: bool,
    pub emit: Emit,
    /// The request or script in flight, shared with `cancel_journey`.
    current: Arc<Mutex<Option<String>>>,
}

impl<'a> EngineHost<'a> {
    pub fn new(
        deps: SendDeps<'a>,
        collection_id: String,
        cancel: Arc<AtomicBool>,
        current: Arc<Mutex<Option<String>>>,
        secrets: Vec<String>,
        scripts_trusted: bool,
        emit: Emit,
    ) -> Self {
        Self {
            deps,
            collection_id,
            cancel,
            secrets,
            scripts_trusted,
            emit,
            current,
        }
    }

    fn location(&self, path: &str) -> RequestLocation {
        RequestLocation::Collection {
            collection_id: self.collection_id.clone(),
            path: path.to_string(),
        }
    }

    fn set_current(&self, id: Option<String>) {
        *self.current.lock().unwrap_or_else(PoisonError::into_inner) = id;
    }

    fn redactor(&self) -> Redactor {
        let secrets = self.secrets.clone();
        Arc::new(move |text: &str| {
            let refs: Vec<&str> = secrets.iter().map(String::as_str).collect();
            mask_secrets(text, &refs)
        })
    }

    fn read_content(&self, location: &RequestLocation) -> Result<RequestContent, String> {
        let doc = self
            .deps
            .store
            .lock_store()
            .read_request(location)
            .map_err(|error| error.message)?;
        match doc.content.kind {
            RequestKind::Http | RequestKind::Graphql | RequestKind::Soap => Ok(doc.content),
            _ => Err("A journey can run HTTP, GraphQL and SOAP requests.".into()),
        }
    }
}

impl Host for EngineHost<'_> {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    async fn sleep(&self, ms: u32) {
        let step = Duration::from_millis(50);
        let mut left = Duration::from_millis(u64::from(ms));
        while !left.is_zero() && !self.cancelled() {
            let wait = left.min(step);
            tokio::time::sleep(wait).await;
            left -= wait;
        }
    }

    async fn send(
        &self,
        request: &str,
        _node_id: &str,
        vars: &BTreeMap<String, String>,
    ) -> Result<Sent, String> {
        let location = self.location(request);
        let content = self.read_content(&location)?;
        let id = uuid::Uuid::new_v4().to_string();
        self.set_current(Some(id.clone()));
        let outcome = run_send_with(&self.deps, &id, &location, &content, vars).await;
        self.set_current(None);
        let outcome = outcome.map_err(|error| (self.redactor())(&error.message))?;
        if let Some(error) = outcome.error {
            return Err((self.redactor())(&error.message));
        }
        let Some(meta) = outcome.meta else {
            let why = outcome
                .scripts
                .iter()
                .find_map(|report| report.error.as_ref().map(|error| error.message.clone()))
                .unwrap_or_else(|| "A script stopped the request.".into());
            return Err((self.redactor())(&why));
        };
        let body = match self.deps.engine.body_file(&id) {
            Ok(path) => read_capped(&path).await,
            Err(_) => Vec::new(),
        };
        Ok(Sent {
            status: meta.status,
            headers: meta
                .headers
                .iter()
                .map(|header| (header.name.clone(), header.value.clone()))
                .collect(),
            body: String::from_utf8_lossy(&body).into_owned(),
        })
    }

    async fn script(
        &self,
        source: &str,
        vars: &BTreeMap<String, String>,
        last: Option<&Sent>,
    ) -> Result<ScriptDone, String> {
        if !self.scripts_trusted {
            return Err(
                "Scripts of this collection do not run until you trust it (collection menu, Trust scripts…)."
                    .into(),
            );
        }
        let id = uuid::Uuid::new_v4().to_string();
        let run = self.deps.engine.script_run(&id);
        self.set_current(Some(id));
        let location = self.location("");
        let draft = RequestContent::default();
        let runner = ChainRunner {
            engine: self.deps.engine,
            location: &location,
            draft: &draft,
            cancel: run.flag.clone(),
            redactor: self.redactor(),
            secrets: self.secrets.clone(),
            strays: run.strays.clone(),
            limits: Limits::default(),
        };
        let mut layers = Layers::default();
        layers.runtime.values.clone_from(vars);
        let mut context = ScriptContext::new(layers, ScriptRequest::default());
        context.response = last.map(|sent| ScriptResponse {
            code: u32::from(sent.status),
            status: String::new(),
            headers: sent.headers.clone(),
            body: sent.body.clone(),
            response_time_ms: 0,
        });
        let scripts = vec![Script::new("journey", source)];
        let (after, report) = runner
            .run(ScriptPhase::PostResponse, scripts, context)
            .await;
        self.set_current(None);
        if let Some(error) = report.error {
            return Err((self.redactor())(&error.message));
        }
        Ok(ScriptDone {
            vars: after.layers.runtime.values,
            console: report.console.into_iter().map(|line| line.text).collect(),
        })
    }

    fn emit(&self, event: RunEvent) {
        (self.emit)(event);
    }

    fn mask(&self, text: &str) -> String {
        (self.redactor())(text)
    }
}
