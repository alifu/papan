//! Walks a journey graph with one variable context (plan 13 §4). Everything outside the
//! graph (sending a request, running a script, sleeping, events) goes through [`Host`], so
//! the walk is tested without a network.

use std::collections::BTreeMap;
use std::future::Future;

use futures_util::future::BoxFuture;

use super::check::{evaluate, extract, fill, Scope, Sent};
use super::data::Row;
use super::model::{
    FailurePolicy, Journey, JourneyEdge, JourneyNode, NodeType, Output, MAX_DELAY_MS, WHEN_DONE,
    WHEN_EACH, WHEN_FALSE, WHEN_TRUE,
};
use super::types::{
    HeaderLine, NodeStatus, ResponseSummary, RunEvent, RunEventKind, RunStatus, RunSummary, VarSet,
};

/// Longest body text an event carries.
const PREVIEW_BYTES: usize = 16 * 1024;
const DEFAULT_LOOP_VAR: &str = "item";

/// What a Script node leaves behind.
#[derive(Debug, Default)]
pub struct ScriptDone {
    pub vars: BTreeMap<String, String>,
    pub console: Vec<String>,
}

/// The outside world of a run.
pub trait Host: Sync {
    fn cancelled(&self) -> bool;
    fn sleep(&self, ms: u32) -> impl Future<Output = ()> + Send;
    /// Sends the saved request `request`; `vars` are the journey variables.
    fn send(
        &self,
        request: &str,
        node_id: &str,
        vars: &BTreeMap<String, String>,
    ) -> impl Future<Output = Result<Sent, String>> + Send;
    fn script(
        &self,
        source: &str,
        vars: &BTreeMap<String, String>,
        last: Option<&Sent>,
    ) -> impl Future<Output = Result<ScriptDone, String>> + Send;
    /// The rows of a Loop node's data file.
    fn rows(&self, file: &str) -> Result<Vec<Row>, String>;
    fn emit(&self, event: RunEvent);
    /// Hides secret values in text that leaves the run (events, messages).
    fn mask(&self, text: &str) -> String {
        text.to_string()
    }
}

/// How a node went.
struct Done {
    status: NodeStatus,
    message: String,
    response: Option<ResponseSummary>,
    vars: Vec<VarSet>,
    console: Vec<String>,
    /// Which edges to follow: a Condition's branch.
    branch: Option<bool>,
}

impl Done {
    fn passed() -> Self {
        Self {
            status: NodeStatus::Passed,
            message: String::new(),
            response: None,
            vars: Vec::new(),
            console: Vec::new(),
            branch: None,
        }
    }

    fn failed(message: impl Into<String>) -> Self {
        Self {
            status: NodeStatus::Failed,
            message: message.into(),
            ..Self::passed()
        }
    }
}

/// What a walk below a node ended with.
enum Flow {
    Done,
    /// The path came back to this Loop node: the iteration ends.
    Back(String),
    Stop,
}

enum Stopped {
    Failed(String),
    Cancelled,
}

struct Walk<'a, H: Host> {
    host: &'a H,
    journey: &'a Journey,
    vars: BTreeMap<String, String>,
    last: Option<Sent>,
    steps: u32,
    failures: u32,
    active_loops: Vec<String>,
    stopped: Option<Stopped>,
}

fn summary_of(sent: &Sent, mask: &dyn Fn(&str) -> String) -> ResponseSummary {
    let mut cut = sent.body.len().min(PREVIEW_BYTES);
    while !sent.body.is_char_boundary(cut) {
        cut -= 1;
    }
    ResponseSummary {
        status: sent.status,
        headers: sent
            .headers
            .iter()
            .map(|(name, value)| HeaderLine {
                name: name.clone(),
                value: mask(value),
            })
            .collect(),
        body_preview: mask(&sent.body[..cut]),
        body_truncated: cut < sent.body.len(),
    }
}

impl<'a, H: Host> Walk<'a, H> {
    fn node(&self, id: &str) -> Option<&'a JourneyNode> {
        self.journey.nodes.iter().find(|node| node.id == id)
    }

    fn edges_of(&self, id: &str, when: &'static [&'static str]) -> Vec<&'a JourneyEdge> {
        self.journey
            .edges
            .iter()
            .filter(|edge| edge.from == id && when.contains(&edge.when.as_str()))
            .collect()
    }

    fn event(&self, kind: RunEventKind, node_id: &str) -> RunEvent {
        RunEvent {
            kind,
            node_id: node_id.to_string(),
            status: None,
            message: String::new(),
            response: None,
            vars: Vec::new(),
            console: Vec::new(),
            steps: self.steps,
            run_status: None,
        }
    }

    fn finish_event(&self, node_id: &str, done: &Done) -> RunEvent {
        let mask = |text: &str| self.host.mask(text);
        RunEvent {
            status: Some(done.status),
            message: mask(&done.message),
            response: done.response.clone(),
            vars: done
                .vars
                .iter()
                .map(|set| VarSet {
                    name: set.name.clone(),
                    value: mask(&set.value),
                })
                .collect(),
            console: done.console.iter().map(|line| mask(line)).collect(),
            ..self.event(RunEventKind::NodeFinished, node_id)
        }
    }

    fn set(&mut self, name: &str, value: String, done: &mut Done) {
        done.vars.push(VarSet {
            name: name.to_string(),
            value: value.clone(),
        });
        self.vars.insert(name.to_string(), value);
    }

    async fn request(&mut self, node: &JourneyNode) -> Done {
        let sent = match self.host.send(&node.request, &node.id, &self.vars).await {
            Ok(sent) => sent,
            Err(problem) => return Done::failed(problem),
        };
        let mut done = Done::passed();
        done.response = Some(summary_of(&sent, &|text| self.host.mask(text)));
        self.last = Some(sent);
        let mut missing = Vec::new();
        for output in &node.outputs {
            let found = {
                let scope = Scope {
                    vars: &self.vars,
                    last: self.last.as_ref(),
                };
                extract(output, &scope)
            };
            match found {
                Some(value) => self.set(&output.name, value, &mut done),
                None => missing.push(output.name.as_str()),
            }
        }
        if !missing.is_empty() {
            done.status = NodeStatus::Failed;
            done.message = format!("Could not read {} from the response.", missing.join(", "));
        }
        done
    }

    async fn script(&mut self, node: &JourneyNode) -> Done {
        match self
            .host
            .script(&node.script, &self.vars, self.last.as_ref())
            .await
        {
            Ok(result) => {
                let mut done = Done::passed();
                for (name, value) in result.vars {
                    if self.vars.get(&name) != Some(&value) {
                        self.set(&name, value, &mut done);
                    }
                }
                done.console = result.console;
                done
            }
            Err(problem) => Done::failed(problem),
        }
    }

    fn check(&self, node: &JourneyNode) -> Result<bool, String> {
        let scope = Scope {
            vars: &self.vars,
            last: self.last.as_ref(),
        };
        evaluate(&node.check, &scope)
    }

    async fn execute(&mut self, node: &JourneyNode) -> Done {
        match node.node_type {
            NodeType::Start | NodeType::End | NodeType::Loop => Done::passed(),
            NodeType::Request => self.request(node).await,
            NodeType::Script => self.script(node).await,
            NodeType::Delay => {
                self.host.sleep(node.delay_ms.min(MAX_DELAY_MS)).await;
                Done::passed()
            }
            NodeType::Condition => match self.check(node) {
                Ok(branch) => Done {
                    branch: Some(branch),
                    message: format!("Took the {branch} branch."),
                    ..Done::passed()
                },
                Err(problem) => Done {
                    branch: Some(false),
                    ..Done::failed(problem)
                },
            },
            NodeType::Assert => match self.check(node) {
                Ok(true) => Done::passed(),
                Ok(false) => Done::failed("The check did not hold."),
                Err(problem) => Done::failed(problem),
            },
        }
    }

    fn policy_of(&self, node: &JourneyNode) -> FailurePolicy {
        node.on_failure.unwrap_or(self.journey.settings.on_failure)
    }

    /// Copies the values an edge maps into variables, before the target node runs.
    fn apply_map(&mut self, via: &[Output]) -> Done {
        let mut done = Done::passed();
        let mut missing = Vec::new();
        for mapping in via {
            let found = {
                let scope = Scope {
                    vars: &self.vars,
                    last: self.last.as_ref(),
                };
                extract(mapping, &scope)
            };
            match found {
                Some(value) => self.set(&mapping.name, value, &mut done),
                None => missing.push(mapping.name.as_str()),
            }
        }
        if !missing.is_empty() {
            done.status = NodeStatus::Failed;
            done.message = format!("Could not map {} onto this node.", missing.join(", "));
        }
        done
    }

    /// The node's own work, after its incoming mappings; their variables come first.
    async fn work(&mut self, node: &JourneyNode, mapped: Done) -> Done {
        if mapped.status == NodeStatus::Failed {
            let branch = (node.node_type == NodeType::Condition).then_some(false);
            return Done { branch, ..mapped };
        }
        let mut done = self.execute(node).await;
        done.vars.splice(0..0, mapped.vars);
        done
    }

    /// Runs `node` (entered through an edge that maps `via`), then everything after it.
    fn walk<'b>(&'b mut self, id: &'b str, via: &'b [Output]) -> BoxFuture<'b, Flow> {
        Box::pin(async move {
            if self.host.cancelled() {
                self.stopped = Some(Stopped::Cancelled);
                return Flow::Stop;
            }
            let Some(node) = self.node(id) else {
                return Flow::Done;
            };
            if self.steps >= self.journey.settings.max_steps {
                let limit = self.journey.settings.max_steps;
                self.stopped = Some(Stopped::Failed(format!(
                    "The run stopped after {limit} steps."
                )));
                return Flow::Stop;
            }
            self.steps += 1;
            self.host.emit(self.event(RunEventKind::NodeStarted, id));
            let mapped = self.apply_map(via);
            if node.node_type == NodeType::Loop && mapped.status == NodeStatus::Passed {
                return self.run_loop(node, mapped).await;
            }
            let done = self.work(node, mapped).await;
            self.settle(node, done).await
        })
    }

    /// Reports how `node` went, applies the failure policy and follows its edges.
    async fn settle(&mut self, node: &'a JourneyNode, done: Done) -> Flow {
        let id = node.id.as_str();
        self.host.emit(self.finish_event(id, &done));
        if self.host.cancelled() {
            self.stopped = Some(Stopped::Cancelled);
            return Flow::Stop;
        }
        if done.status == NodeStatus::Failed {
            self.failures += 1;
            if self.policy_of(node) == FailurePolicy::Stop {
                self.stopped = Some(Stopped::Failed(self.host.mask(&done.message)));
                return Flow::Stop;
            }
        }
        let when: &'static [&'static str] = match (done.branch, node.node_type) {
            (Some(true), _) => &[WHEN_TRUE],
            (Some(false), _) => &[WHEN_FALSE],
            (None, NodeType::Loop) => &[WHEN_DONE],
            (None, _) => &[""],
        };
        self.follow(self.edges_of(id, when)).await
    }

    /// Walks the targets of `edges` one after the other.
    async fn follow(&mut self, edges: Vec<&'a JourneyEdge>) -> Flow {
        let mut back = None;
        for edge in edges {
            if self.active_loops.iter().any(|open| open == &edge.to) {
                back = back.or(Some(edge.to.clone()));
                continue;
            }
            match self.walk(&edge.to, &edge.map).await {
                Flow::Stop => return Flow::Stop,
                Flow::Back(target) => back = back.or(Some(target)),
                Flow::Done => {}
            }
        }
        back.map_or(Flow::Done, Flow::Back)
    }

    /// What each iteration of a Loop sets: a data row's columns, an item, or the count.
    fn iterations(&self, node: &JourneyNode) -> Result<Vec<Vec<(String, String)>>, String> {
        let var = if node.loop_var.is_empty() {
            DEFAULT_LOOP_VAR
        } else {
            node.loop_var.as_str()
        };
        let sets: Vec<Vec<(String, String)>> = if !node.data_file.is_empty() {
            self.host
                .rows(&node.data_file)?
                .into_iter()
                .map(|row| row.into_iter().collect())
                .collect()
        } else if node.items.is_empty() {
            (0..node.count)
                .map(|n| vec![(var.to_string(), n.to_string())])
                .collect()
        } else {
            node.items
                .iter()
                .map(|item| vec![(var.to_string(), fill(item, &self.vars))])
                .collect()
        };
        Ok(sets
            .into_iter()
            .enumerate()
            .map(|(index, mut set)| {
                set.push((format!("{var}_index"), index.to_string()));
                set
            })
            .collect())
    }

    async fn run_loop(&mut self, node: &'a JourneyNode, mapped: Done) -> Flow {
        let iterations = match self.iterations(node) {
            Ok(iterations) => iterations,
            Err(problem) => {
                let done = Done {
                    vars: mapped.vars,
                    ..Done::failed(problem)
                };
                return self.settle(node, done).await;
            }
        };
        self.active_loops.push(node.id.clone());
        let mut escaped = None;
        for set in &iterations {
            for (name, value) in set {
                self.vars.insert(name.clone(), value.clone());
            }
            match self.follow(self.edges_of(&node.id, &[WHEN_EACH])).await {
                Flow::Stop => {
                    self.active_loops.pop();
                    return Flow::Stop;
                }
                // A path went back to an outer loop: this one ends too.
                Flow::Back(target) if target != node.id => {
                    escaped = Some(target);
                    break;
                }
                _ => {}
            }
        }
        self.active_loops.pop();
        let mut done = Done::passed();
        done.vars = mapped.vars;
        done.message = format!("{} iteration(s).", iterations.len());
        self.host.emit(self.finish_event(&node.id, &done));
        if let Some(target) = escaped {
            return Flow::Back(target);
        }
        self.follow(self.edges_of(&node.id, &[WHEN_DONE])).await
    }
}

/// Runs `journey` from its Start node.
pub async fn run<H: Host>(host: &H, journey: &Journey, run_id: &str) -> RunSummary {
    let mut walk = Walk {
        host,
        journey,
        vars: BTreeMap::new(),
        last: None,
        steps: 0,
        failures: 0,
        active_loops: Vec::new(),
        stopped: None,
    };
    let start = journey
        .nodes
        .iter()
        .find(|node| node.node_type == NodeType::Start);
    let (status, message) = match start {
        None => (
            RunStatus::Failed,
            "The journey has no Start node.".to_string(),
        ),
        Some(start) => {
            walk.walk(&start.id, &[]).await;
            match walk.stopped.take() {
                Some(Stopped::Cancelled) => (RunStatus::Cancelled, "The run was stopped.".into()),
                Some(Stopped::Failed(problem)) => (RunStatus::Failed, problem),
                None if walk.failures > 0 => (
                    RunStatus::Failed,
                    format!("{} step(s) failed.", walk.failures),
                ),
                None => (RunStatus::Passed, String::new()),
            }
        }
    };
    let mut last = walk.event(RunEventKind::RunFinished, "");
    last.message.clone_from(&message);
    last.run_status = Some(status);
    host.emit(last);
    RunSummary {
        run_id: run_id.to_string(),
        status,
        steps: walk.steps,
        failures: walk.failures,
        message,
    }
}
