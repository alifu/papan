use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use super::check::Sent;
use super::data::Row;
use super::model::tests_support::{edge, request};
use super::model::{
    Check, CheckOp, CheckSource, FailurePolicy, Journey, JourneyNode, JourneySettings, NodeType,
    Output,
};
use super::runner::{run, Host, ScriptDone};
use super::types::{NodeStatus, RunEvent, RunEventKind, RunStatus};

/// Answers requests from queues by request path and records what it was asked.
#[derive(Default)]
struct Fake {
    answers: Mutex<HashMap<String, VecDeque<Result<Sent, String>>>>,
    asked: Mutex<Vec<(String, BTreeMap<String, String>)>>,
    events: Mutex<Vec<RunEvent>>,
    sends: AtomicUsize,
    cancel_after: Option<usize>,
    script_sets: Vec<(String, String)>,
    files: HashMap<String, Result<Vec<Row>, String>>,
    run_limit: Option<std::time::Duration>,
}

impl Fake {
    fn answer(self, path: &str, status: u16, body: &str) -> Self {
        self.answers
            .lock()
            .unwrap()
            .entry(path.into())
            .or_default()
            .push_back(Ok(Sent {
                status,
                headers: vec![("X-Id".into(), "h1".into())],
                body: body.into(),
            }));
        self
    }

    fn asked(&self) -> Vec<String> {
        self.asked
            .lock()
            .unwrap()
            .iter()
            .map(|(p, _)| p.clone())
            .collect()
    }
}

impl Host for Fake {
    fn cancelled(&self) -> bool {
        self.cancel_after
            .is_some_and(|limit| self.sends.load(Ordering::SeqCst) >= limit)
    }

    async fn sleep(&self, _ms: u32) {}

    async fn send(
        &self,
        request: &str,
        _node_id: &str,
        vars: &BTreeMap<String, String>,
    ) -> Result<Sent, String> {
        self.sends.fetch_add(1, Ordering::SeqCst);
        self.asked
            .lock()
            .unwrap()
            .push((request.into(), vars.clone()));
        let mut answers = self.answers.lock().unwrap();
        let queue = answers.get_mut(request).ok_or("no answer queued")?;
        match queue.len() {
            0 => Err("no answer queued".into()),
            1 => queue[0].clone(),
            _ => queue.pop_front().unwrap(),
        }
    }

    async fn script(
        &self,
        source: &str,
        vars: &BTreeMap<String, String>,
        _last: Option<&Sent>,
    ) -> Result<ScriptDone, String> {
        if source == "boom" {
            return Err("script failed".into());
        }
        let mut next = vars.clone();
        next.extend(self.script_sets.iter().cloned());
        Ok(ScriptDone {
            vars: next,
            console: vec!["hello".into()],
        })
    }

    fn rows(&self, file: &str) -> Result<Vec<Row>, String> {
        self.files
            .get(file)
            .cloned()
            .unwrap_or_else(|| Err("no such data file".into()))
    }

    fn emit(&self, event: RunEvent) {
        self.events.lock().unwrap().push(event);
    }

    fn run_limit(&self) -> std::time::Duration {
        self.run_limit.unwrap_or(super::runner::MAX_RUN)
    }
}

fn node(id: &str, node_type: NodeType) -> JourneyNode {
    JourneyNode::new(id, node_type)
}

fn assert_status(id: &str, op: CheckOp, value: &str) -> JourneyNode {
    JourneyNode {
        check: Check {
            source: CheckSource::Status,
            name: String::new(),
            op,
            value: value.into(),
        },
        ..node(id, NodeType::Assert)
    }
}

fn journey(nodes: Vec<JourneyNode>, edges: Vec<(&str, &str, &str)>) -> Journey {
    Journey {
        name: "t".into(),
        nodes,
        edges: edges
            .into_iter()
            .enumerate()
            .map(|(n, (from, to, when))| edge(&format!("e{n}"), from, to, when))
            .collect(),
        settings: JourneySettings::default(),
    }
}

#[tokio::test]
async fn a_token_from_one_request_reaches_the_next() {
    let login = JourneyNode {
        outputs: vec![Output {
            name: "token".into(),
            source: CheckSource::Body,
            path: "data.token".into(),
        }],
        ..request("login", "login")
    };
    let j = journey(
        vec![
            node("s", NodeType::Start),
            login,
            request("me", "me"),
            assert_status("ok", CheckOp::Eq, "200"),
            node("e", NodeType::End),
        ],
        vec![
            ("s", "login", ""),
            ("login", "me", ""),
            ("me", "ok", ""),
            ("ok", "e", ""),
        ],
    );
    let host = Fake::default()
        .answer("login", 200, r#"{"data": {"token": "abc"}}"#)
        .answer("me", 200, "{}");
    let done = run(&host, &j, "r1").await;
    assert_eq!(
        (done.status, done.steps, done.failures),
        (RunStatus::Passed, 5, 0)
    );
    let asked = host.asked.lock().unwrap();
    assert_eq!(asked[1].1.get("token").map(String::as_str), Some("abc"));
    let events = host.events.lock().unwrap();
    assert_eq!(events.last().unwrap().kind, RunEventKind::RunFinished);
    let login_done = events
        .iter()
        .find(|e| e.node_id == "login" && e.kind == RunEventKind::NodeFinished)
        .unwrap();
    assert_eq!(login_done.vars[0].name, "token");
    assert_eq!(login_done.response.as_ref().unwrap().status, 200);
}

#[tokio::test]
async fn a_condition_follows_only_its_branch() {
    let cond = JourneyNode {
        check: Check {
            source: CheckSource::Status,
            name: String::new(),
            op: CheckOp::Eq,
            value: "404".into(),
        },
        ..node("c", NodeType::Condition)
    };
    let j = journey(
        vec![
            node("s", NodeType::Start),
            request("r", "r"),
            cond,
            request("yes", "yes"),
            request("no", "no"),
        ],
        vec![
            ("s", "r", ""),
            ("r", "c", ""),
            ("c", "yes", "true"),
            ("c", "no", "false"),
        ],
    );
    let host = Fake::default()
        .answer("r", 404, "")
        .answer("yes", 200, "")
        .answer("no", 200, "");
    assert_eq!(run(&host, &j, "r").await.status, RunStatus::Passed);
    assert_eq!(host.asked(), ["r", "yes"]);
}

#[tokio::test]
async fn a_failed_assert_stops_or_continues_by_policy() {
    let build = |policy: FailurePolicy| {
        let mut j = journey(
            vec![
                node("s", NodeType::Start),
                request("r", "r"),
                assert_status("a", CheckOp::Eq, "200"),
                request("after", "after"),
            ],
            vec![("s", "r", ""), ("r", "a", ""), ("a", "after", "")],
        );
        j.settings.on_failure = policy;
        j
    };
    let host = Fake::default()
        .answer("r", 500, "")
        .answer("after", 200, "");
    let stopped = run(&host, &build(FailurePolicy::Stop), "r").await;
    assert_eq!((stopped.status, stopped.failures), (RunStatus::Failed, 1));
    assert_eq!(host.asked(), ["r"]);

    let host = Fake::default()
        .answer("r", 500, "")
        .answer("after", 200, "");
    let went_on = run(&host, &build(FailurePolicy::Continue), "r").await;
    assert_eq!((went_on.status, went_on.failures), (RunStatus::Failed, 1));
    assert_eq!(host.asked(), ["r", "after"]);

    let mut single = build(FailurePolicy::Stop);
    single.nodes[2].on_failure = Some(FailurePolicy::Continue);
    let host = Fake::default()
        .answer("r", 500, "")
        .answer("after", 200, "");
    run(&host, &single, "r").await;
    assert_eq!(host.asked(), ["r", "after"]);
}

#[tokio::test]
async fn a_missing_output_fails_the_request_node() {
    let r = JourneyNode {
        outputs: vec![Output {
            name: "id".into(),
            source: CheckSource::Body,
            path: "id".into(),
        }],
        ..request("r", "r")
    };
    let j = journey(vec![node("s", NodeType::Start), r], vec![("s", "r", "")]);
    let host = Fake::default().answer("r", 200, "{}");
    let done = run(&host, &j, "r").await;
    assert_eq!(done.status, RunStatus::Failed);
    assert!(done.message.contains("id"), "{}", done.message);
}

#[tokio::test]
async fn a_loop_runs_its_body_per_item_then_continues() {
    let looped = JourneyNode {
        items: vec!["a".into(), "b".into(), "c".into()],
        loop_var: "who".into(),
        ..node("l", NodeType::Loop)
    };
    let j = journey(
        vec![
            node("s", NodeType::Start),
            looped,
            request("body", "body"),
            request("fin", "fin"),
        ],
        vec![
            ("s", "l", ""),
            ("l", "body", "each"),
            ("body", "l", ""),
            ("l", "fin", "done"),
        ],
    );
    let host = Fake::default()
        .answer("body", 200, "")
        .answer("fin", 200, "");
    let done = run(&host, &j, "r").await;
    assert_eq!(done.status, RunStatus::Passed);
    assert_eq!(host.asked(), ["body", "body", "body", "fin"]);
    let asked = host.asked.lock().unwrap();
    let seen: Vec<_> = asked[..3]
        .iter()
        .map(|(_, v)| (v["who"].clone(), v["who_index"].clone()))
        .collect();
    assert_eq!(
        seen,
        [
            ("a".into(), "0".into()),
            ("b".into(), "1".into()),
            ("c".into(), "2".into())
        ]
    );
}

#[tokio::test]
async fn a_loop_by_count_with_nothing_to_go_back_to_ends_each_iteration() {
    let looped = JourneyNode {
        count: 2,
        ..node("l", NodeType::Loop)
    };
    let j = journey(
        vec![node("s", NodeType::Start), looped, request("body", "body")],
        vec![("s", "l", ""), ("l", "body", "each")],
    );
    let host = Fake::default().answer("body", 200, "");
    run(&host, &j, "r").await;
    assert_eq!(host.asked(), ["body", "body"]);
}

#[tokio::test]
async fn the_step_limit_ends_a_long_run() {
    let looped = JourneyNode {
        count: 1000,
        ..node("l", NodeType::Loop)
    };
    let mut j = journey(
        vec![node("s", NodeType::Start), looped, request("body", "body")],
        vec![("s", "l", ""), ("l", "body", "each"), ("body", "l", "")],
    );
    j.settings.max_steps = 10;
    let host = Fake::default().answer("body", 200, "");
    let done = run(&host, &j, "r").await;
    assert_eq!(done.status, RunStatus::Failed);
    assert!(done.message.contains("10 steps"), "{}", done.message);
    assert_eq!(done.steps, 10);
}

#[tokio::test]
async fn cancel_stops_the_run() {
    let j = journey(
        vec![
            node("s", NodeType::Start),
            request("a", "a"),
            request("b", "b"),
        ],
        vec![("s", "a", ""), ("a", "b", "")],
    );
    let host = Fake {
        cancel_after: Some(1),
        ..Fake::default()
    }
    .answer("a", 200, "")
    .answer("b", 200, "");
    let done = run(&host, &j, "r").await;
    assert_eq!(done.status, RunStatus::Cancelled);
    assert_eq!(host.asked(), ["a"]);
}

#[tokio::test]
async fn scripts_set_variables_and_failures_are_reported() {
    let script = JourneyNode {
        script: "ok".into(),
        ..node("sc", NodeType::Script)
    };
    let j = journey(
        vec![node("s", NodeType::Start), script, request("r", "r")],
        vec![("s", "sc", ""), ("sc", "r", "")],
    );
    let host = Fake {
        script_sets: vec![("k".into(), "v".into())],
        ..Fake::default()
    }
    .answer("r", 200, "");
    assert_eq!(run(&host, &j, "r").await.status, RunStatus::Passed);
    assert_eq!(host.asked.lock().unwrap()[0].1["k"], "v");
    {
        let events = host.events.lock().unwrap();
        let finished = events
            .iter()
            .find(|e| e.node_id == "sc" && e.kind == RunEventKind::NodeFinished)
            .unwrap();
        assert_eq!(finished.console, ["hello"]);
    }

    let bad = JourneyNode {
        script: "boom".into(),
        ..node("sc", NodeType::Script)
    };
    let j = journey(vec![node("s", NodeType::Start), bad], vec![("s", "sc", "")]);
    let done = run(&Fake::default(), &j, "r").await;
    assert_eq!(
        (done.status, done.message.as_str()),
        (RunStatus::Failed, "script failed")
    );
}

#[tokio::test]
async fn a_journey_needs_a_start_node() {
    let j = journey(vec![request("r", "r")], vec![]);
    let done = run(&Fake::default(), &j, "r").await;
    assert_eq!(done.status, RunStatus::Failed);
    let first = Fake::default();
    run(&first, &j, "r").await;
    assert_eq!(first.events.lock().unwrap().len(), 1);
    let _ = NodeStatus::Passed;
}

fn token_output() -> Output {
    Output {
        name: "t".into(),
        source: CheckSource::Body,
        path: "data.token".into(),
    }
}

fn row(pairs: &[(&str, &str)]) -> Row {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

#[tokio::test]
async fn an_edge_mapping_copies_a_response_value_into_a_variable_for_the_next_node() {
    let mut j = journey(
        vec![
            node("s", NodeType::Start),
            request("login", "login"),
            request("use", "use"),
        ],
        vec![("s", "login", ""), ("login", "use", "")],
    );
    j.edges[1].map = vec![Output {
        name: "auth".into(),
        ..token_output()
    }];
    let host = Fake::default()
        .answer("login", 200, r#"{"data":{"token":"abc"}}"#)
        .answer("use", 200, "{}");
    let done = run(&host, &j, "r").await;
    assert_eq!(done.status, RunStatus::Passed);
    let asked = host.asked.lock().unwrap();
    assert_eq!(asked[1].1.get("auth").map(String::as_str), Some("abc"));
    assert!(!asked[0].1.contains_key("auth"));
    let events = host.events.lock().unwrap();
    let finished = events
        .iter()
        .find(|e| e.kind == RunEventKind::NodeFinished && e.node_id == "use")
        .unwrap();
    assert_eq!(finished.vars[0].name, "auth");
}

#[tokio::test]
async fn a_mapping_that_finds_nothing_fails_the_target_before_it_runs() {
    let mut j = journey(
        vec![
            node("s", NodeType::Start),
            request("login", "login"),
            request("use", "use"),
        ],
        vec![("s", "login", ""), ("login", "use", "")],
    );
    j.edges[1].map = vec![token_output()];
    let host = Fake::default()
        .answer("login", 200, r#"{"other":1}"#)
        .answer("use", 200, "{}");
    let done = run(&host, &j, "r").await;
    assert_eq!(done.status, RunStatus::Failed);
    assert_eq!(host.asked(), ["login"]);
    let events = host.events.lock().unwrap();
    let finished = events
        .iter()
        .find(|e| e.kind == RunEventKind::NodeFinished && e.node_id == "use")
        .unwrap();
    assert_eq!(finished.status, Some(NodeStatus::Failed));
    assert!(finished.message.contains('t'));
}

fn data_loop(file: &str) -> Journey {
    let looped = JourneyNode {
        data_file: file.into(),
        loop_var: "row".into(),
        ..node("loop", NodeType::Loop)
    };
    journey(
        vec![
            node("s", NodeType::Start),
            looped,
            request("body", "body"),
            request("after", "after"),
        ],
        vec![
            ("s", "loop", ""),
            ("loop", "body", "each"),
            ("body", "loop", ""),
            ("loop", "after", "done"),
        ],
    )
}

#[tokio::test]
async fn a_loop_runs_once_per_data_row_with_the_columns_as_variables() {
    let mut host = Fake::default()
        .answer("body", 200, "{}")
        .answer("after", 200, "{}");
    host.files.insert(
        "users.csv".into(),
        Ok(vec![
            row(&[("name", "ann"), ("age", "3")]),
            row(&[("name", "bob"), ("age", "4")]),
        ]),
    );
    let done = run(&host, &data_loop("users.csv"), "r").await;
    assert_eq!(done.status, RunStatus::Passed);
    assert_eq!(host.asked(), ["body", "body", "after"]);
    let asked = host.asked.lock().unwrap();
    assert_eq!(asked[0].1.get("name").map(String::as_str), Some("ann"));
    assert_eq!(asked[1].1.get("name").map(String::as_str), Some("bob"));
    assert_eq!(asked[1].1.get("row_index").map(String::as_str), Some("1"));
}

#[tokio::test]
async fn a_column_a_row_lacks_does_not_keep_the_value_of_the_row_before() {
    let mut host = Fake::default()
        .answer("body", 200, "{}")
        .answer("after", 200, "{}");
    host.files.insert(
        "users.json".into(),
        Ok(vec![
            row(&[("name", "ann"), ("nick", "annie")]),
            row(&[("name", "bob")]),
        ]),
    );
    let done = run(&host, &data_loop("users.json"), "r").await;
    assert_eq!(done.status, RunStatus::Passed);
    let asked = host.asked.lock().unwrap();
    assert_eq!(asked[0].1.get("nick").map(String::as_str), Some("annie"));
    assert_eq!(asked[1].1.get("nick"), None);
    assert_eq!(asked[1].1.get("name").map(String::as_str), Some("bob"));
}

#[tokio::test]
async fn a_run_that_outlasts_its_time_limit_is_stopped() {
    let j = journey(
        vec![node("s", NodeType::Start), request("a", "a")],
        vec![("s", "a", "")],
    );
    let host = Fake {
        run_limit: Some(std::time::Duration::ZERO),
        ..Fake::default()
    }
    .answer("a", 200, "");
    let done = run(&host, &j, "r").await;
    assert_eq!(done.status, RunStatus::Failed);
    assert!(done.message.contains("minutes"), "{}", done.message);
    assert!(host.asked().is_empty());
}

#[tokio::test]
async fn a_variable_is_cut_to_its_size_limit() {
    let j = journey(
        vec![
            node("s", NodeType::Start),
            JourneyNode {
                script: "x".into(),
                ..node("sc", NodeType::Script)
            },
            request("a", "a"),
        ],
        vec![("s", "sc", ""), ("sc", "a", "")],
    );
    let host = Fake {
        script_sets: vec![("big".into(), "x".repeat(200 * 1024))],
        ..Fake::default()
    }
    .answer("a", 200, "");
    let done = run(&host, &j, "r").await;
    assert_eq!(done.status, RunStatus::Passed);
    let asked = host.asked.lock().unwrap();
    assert_eq!(asked[0].1["big"].len(), 64 * 1024);
}

#[tokio::test]
async fn a_data_file_that_cannot_be_read_fails_the_loop() {
    let host = Fake::default().answer("body", 200, "{}");
    let done = run(&host, &data_loop("missing.csv"), "r").await;
    assert_eq!(done.status, RunStatus::Failed);
    assert!(host.asked().is_empty());
    assert!(done.message.contains("data file"));
}

#[tokio::test]
async fn a_failed_data_loop_goes_on_to_done_when_the_policy_is_continue() {
    let mut j = data_loop("missing.csv");
    j.settings.on_failure = FailurePolicy::Continue;
    let host = Fake::default()
        .answer("body", 200, "{}")
        .answer("after", 200, "{}");
    let done = run(&host, &j, "r").await;
    assert_eq!(done.status, RunStatus::Failed);
    assert_eq!(host.asked(), ["after"]);
}
