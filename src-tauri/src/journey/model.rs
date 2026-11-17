//! A journey as stored in `journeys/<slug>.journey.yaml` and sent over IPC (plan 13 §3).
//! The graph is flat on purpose: one node struct with the fields of every node type, so a
//! file diffs well and the frontend needs no tagged unions.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::AppError;

pub const MAX_NODES: usize = 500;
pub const MAX_EDGES: usize = 2_000;
pub const MAX_NAME_CHARS: usize = 100;
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_OUTPUTS: usize = 50;
pub const MAX_LOOP_ITEMS: usize = 1_000;
pub const MAX_DELAY_MS: u32 = 60_000;
pub const DEFAULT_MAX_STEPS: u32 = 1_000;
pub const LIMIT_MAX_STEPS: u32 = 10_000;
/// The branch labels of edges.
pub const WHEN_TRUE: &str = "true";
pub const WHEN_FALSE: &str = "false";
pub const WHEN_EACH: &str = "each";
pub const WHEN_DONE: &str = "done";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    Start,
    End,
    Request,
    Script,
    Condition,
    Delay,
    Assert,
    Loop,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FailurePolicy {
    /// A failed node ends the run.
    #[default]
    Stop,
    /// The failure is recorded and the run goes on.
    Continue,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CheckSource {
    /// The status code of the last response.
    #[default]
    Status,
    /// A header of the last response; `name` is the header.
    Header,
    /// A JSON path into the last response body; `name` is the path.
    Body,
    /// A journey variable; `name` is the variable.
    Variable,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CheckOp {
    #[default]
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Contains,
    Exists,
    NotExists,
}

/// What a Condition or Assert node tests. `value` may use `{{variables}}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Check {
    #[serde(default)]
    pub source: CheckSource,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub op: CheckOp,
    #[serde(default)]
    pub value: String,
}

/// A variable a Request node reads from its response and keeps for the nodes after it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Output {
    pub name: String,
    #[serde(default)]
    pub source: CheckSource,
    /// Header name or JSON path, by `source`.
    #[serde(default)]
    pub path: String,
}

/// A node with the fields of every type; each type reads the ones it needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct JourneyNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    /// Request nodes: the saved request, relative to `requests/`.
    #[serde(default)]
    pub request: String,
    /// Request nodes: variables to keep from the response.
    #[serde(default)]
    pub outputs: Vec<Output>,
    /// Script nodes: JavaScript with the `pm` API; `pm.variables` become journey variables.
    #[serde(default)]
    pub script: String,
    /// Condition and Assert nodes.
    #[serde(default)]
    pub check: Check,
    /// Delay nodes.
    #[serde(default)]
    pub delay_ms: u32,
    /// Loop nodes: runs once per item; with no items, `count` times.
    #[serde(default)]
    pub items: Vec<String>,
    #[serde(default)]
    pub count: u32,
    /// Loop nodes: the variable that holds the item (the index is `<var>_index`).
    #[serde(default)]
    pub loop_var: String,
    /// Loop nodes: a `.csv` (first row names the columns) or `.json` (array of objects) file
    /// of the collection; runs once per row, each column a variable.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub data_file: String,
    /// Overrides the journey's policy for this node.
    #[serde(default)]
    pub on_failure: Option<FailurePolicy>,
}

impl JourneyNode {
    pub fn new(id: &str, node_type: NodeType) -> Self {
        Self {
            id: id.into(),
            node_type,
            label: String::new(),
            x: 0.0,
            y: 0.0,
            request: String::new(),
            outputs: Vec::new(),
            script: String::new(),
            check: Check::default(),
            delay_ms: 0,
            items: Vec::new(),
            count: 0,
            loop_var: String::new(),
            data_file: String::new(),
            on_failure: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct JourneyEdge {
    pub id: String,
    pub from: String,
    pub to: String,
    /// `true`/`false` from a Condition, `each`/`done` from a Loop, empty otherwise.
    #[serde(default)]
    pub when: String,
    /// Values copied into variables as the target node starts (plan 13c): each reads the
    /// last response or a variable and keeps it under `name`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub map: Vec<Output>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct JourneySettings {
    #[serde(default)]
    pub on_failure: FailurePolicy,
    #[serde(default = "default_max_steps")]
    pub max_steps: u32,
}

fn default_max_steps() -> u32 {
    DEFAULT_MAX_STEPS
}

impl Default for JourneySettings {
    fn default() -> Self {
        Self {
            on_failure: FailurePolicy::Stop,
            max_steps: DEFAULT_MAX_STEPS,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct Journey {
    pub name: String,
    #[serde(default)]
    pub nodes: Vec<JourneyNode>,
    #[serde(default)]
    pub edges: Vec<JourneyEdge>,
    #[serde(default)]
    pub settings: JourneySettings,
}

fn invalid(file: &str, field: &str, problem: &str) -> AppError {
    AppError::invalid_input(format!("{file}: {field} {problem}."))
}

fn check_text(file: &str, field: &str, text: &str, max: usize) -> Result<(), AppError> {
    if text.len() > max {
        return Err(invalid(file, field, "is too long"));
    }
    Ok(())
}

fn check_check(file: &str, at: &str, check: &Check) -> Result<(), AppError> {
    check_text(
        file,
        &format!("{at}.check.name"),
        &check.name,
        MAX_NAME_CHARS * 4,
    )?;
    check_text(
        file,
        &format!("{at}.check.value"),
        &check.value,
        MAX_TEXT_BYTES,
    )?;
    let needs_name = !matches!(check.source, CheckSource::Status);
    if needs_name && check.name.trim().is_empty() {
        return Err(invalid(file, &format!("{at}.check.name"), "is required"));
    }
    Ok(())
}

fn check_node(
    file: &str,
    n: usize,
    node: &JourneyNode,
    ids: &mut HashSet<String>,
) -> Result<(), AppError> {
    let at = format!("nodes[{n}]");
    if node.id.is_empty() || node.id.len() > MAX_NAME_CHARS || !ids.insert(node.id.clone()) {
        return Err(invalid(
            file,
            &format!("{at}.id"),
            "is empty, too long or repeated",
        ));
    }
    check_text(
        file,
        &format!("{at}.label"),
        &node.label,
        MAX_NAME_CHARS * 4,
    )?;
    check_text(file, &format!("{at}.script"), &node.script, MAX_TEXT_BYTES)?;
    if !node.x.is_finite() || !node.y.is_finite() {
        return Err(invalid(file, &format!("{at}.position"), "is not a number"));
    }
    match node.node_type {
        NodeType::Request => {
            if !is_safe_relative(&node.request) {
                return Err(invalid(
                    file,
                    &format!("{at}.request"),
                    "is not a request path",
                ));
            }
            if node.outputs.len() > MAX_OUTPUTS {
                return Err(invalid(
                    file,
                    &format!("{at}.outputs"),
                    "has too many outputs",
                ));
            }
            for (k, output) in node.outputs.iter().enumerate() {
                check_output(file, &format!("{at}.outputs[{k}]"), output)?;
            }
        }
        NodeType::Condition | NodeType::Assert => check_check(file, &at, &node.check)?,
        NodeType::Delay if node.delay_ms > MAX_DELAY_MS => {
            return Err(invalid(file, &format!("{at}.delay_ms"), "is over 60000"));
        }
        NodeType::Loop => {
            check_data_file(file, &at, node)?;
            if node.items.len() > MAX_LOOP_ITEMS || node.count as usize > MAX_LOOP_ITEMS {
                return Err(invalid(file, &format!("{at}.count"), "is over 1000"));
            }
            if node.items.iter().any(|item| item.len() > MAX_TEXT_BYTES) {
                return Err(invalid(
                    file,
                    &format!("{at}.items"),
                    "has an item that is too long",
                ));
            }
            if !node.loop_var.is_empty()
                && !node
                    .loop_var
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                return Err(invalid(
                    file,
                    &format!("{at}.loop_var"),
                    "is not a variable name",
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

/// A variable name an output may set.
fn check_output(file: &str, field: &str, output: &Output) -> Result<(), AppError> {
    let name_ok = !output.name.is_empty()
        && output.name.len() <= MAX_NAME_CHARS
        && output
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.');
    if !name_ok {
        return Err(invalid(
            file,
            &format!("{field}.name"),
            "is not a variable name",
        ));
    }
    if matches!(output.source, CheckSource::Variable) {
        return Err(invalid(
            file,
            &format!("{field}.source"),
            "must read the response",
        ));
    }
    check_text(
        file,
        &format!("{field}.path"),
        &output.path,
        MAX_NAME_CHARS * 4,
    )
}

/// A path inside the collection: relative, forward slashes, no `.` or `..`.
pub fn is_safe_relative(path: &str) -> bool {
    let escapes = path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path.split('/').any(|part| part == ".." || part == ".");
    !path.trim().is_empty() && !escapes && path.len() <= 512
}

fn check_data_file(file: &str, at: &str, node: &JourneyNode) -> Result<(), AppError> {
    if node.data_file.is_empty() {
        return Ok(());
    }
    let lower = node.data_file.to_ascii_lowercase();
    let kind_ok = lower.ends_with(".csv") || lower.ends_with(".json");
    if !is_safe_relative(&node.data_file) || !kind_ok {
        return Err(invalid(
            file,
            &format!("{at}.data_file"),
            "must be a .csv or .json file inside the collection",
        ));
    }
    if !node.items.is_empty() {
        return Err(invalid(
            file,
            &format!("{at}.data_file"),
            "cannot be used together with items",
        ));
    }
    Ok(())
}

fn check_edges(
    file: &str,
    journey: &Journey,
    kinds: &HashMap<&str, NodeType>,
) -> Result<(), AppError> {
    let mut seen = HashSet::new();
    for (n, edge) in journey.edges.iter().enumerate() {
        let at = format!("edges[{n}]");
        if edge.id.is_empty() || edge.id.len() > MAX_NAME_CHARS || !seen.insert(edge.id.as_str()) {
            return Err(invalid(
                file,
                &format!("{at}.id"),
                "is empty, too long or repeated",
            ));
        }
        let (Some(from), Some(_)) = (kinds.get(edge.from.as_str()), kinds.get(edge.to.as_str()))
        else {
            return Err(invalid(file, &at, "joins a node that does not exist"));
        };
        let allowed: &[&str] = match from {
            NodeType::Condition => &[WHEN_TRUE, WHEN_FALSE],
            NodeType::Loop => &[WHEN_EACH, WHEN_DONE],
            _ => &[""],
        };
        if !allowed.contains(&edge.when.as_str()) {
            return Err(invalid(
                file,
                &format!("{at}.when"),
                "is not a branch of that node",
            ));
        }
        if edge.map.len() > MAX_OUTPUTS {
            return Err(invalid(file, &format!("{at}.map"), "has too many mappings"));
        }
        for (k, mapping) in edge.map.iter().enumerate() {
            check_output(file, &format!("{at}.map[{k}]"), mapping)?;
        }
        if *from == NodeType::End || edge.from == edge.to {
            return Err(invalid(
                file,
                &at,
                "leaves an End node or joins a node to itself",
            ));
        }
    }
    Ok(())
}

/// Every cycle must pass through a Loop node: without the edges that lead into Loop
/// nodes, the graph has none.
fn check_acyclic(
    file: &str,
    journey: &Journey,
    kinds: &HashMap<&str, NodeType>,
) -> Result<(), AppError> {
    let mut next: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in &journey.edges {
        if kinds.get(edge.to.as_str()) != Some(&NodeType::Loop) {
            next.entry(edge.from.as_str())
                .or_default()
                .push(edge.to.as_str());
        }
    }
    // 0 = new, 1 = on the path, 2 = done.
    let mut state: HashMap<&str, u8> = HashMap::new();
    for node in &journey.nodes {
        let mut stack = vec![(node.id.as_str(), 0_usize)];
        while let Some((id, at)) = stack.pop() {
            if at == 0 {
                match state.get(id) {
                    Some(2) => continue,
                    Some(_) => {
                        return Err(invalid(
                            file,
                            "edges",
                            "make a cycle that does not pass through a Loop node",
                        ))
                    }
                    None => {
                        state.insert(id, 1);
                    }
                }
            }
            match next.get(id).and_then(|list| list.get(at)) {
                Some(child) => {
                    stack.push((id, at + 1));
                    if state.get(child) == Some(&1) {
                        return Err(invalid(
                            file,
                            "edges",
                            "make a cycle that does not pass through a Loop node",
                        ));
                    }
                    if state.get(child) != Some(&2) {
                        stack.push((child, 0));
                    }
                }
                None => {
                    state.insert(id, 2);
                }
            }
        }
    }
    Ok(())
}

/// Checks a journey read from a file or sent from the webview.
pub fn validate(file: &str, journey: &Journey) -> Result<(), AppError> {
    if journey.name.trim().is_empty() {
        return Err(invalid(file, "name", "is required"));
    }
    check_text(file, "name", &journey.name, MAX_NAME_CHARS * 4)?;
    if journey.nodes.len() > MAX_NODES {
        return Err(invalid(file, "nodes", "has too many nodes"));
    }
    if journey.edges.len() > MAX_EDGES {
        return Err(invalid(file, "edges", "has too many edges"));
    }
    if journey.settings.max_steps == 0 || journey.settings.max_steps > LIMIT_MAX_STEPS {
        return Err(invalid(file, "settings.max_steps", "must be 1 to 10000"));
    }
    let mut ids = HashSet::new();
    for (n, node) in journey.nodes.iter().enumerate() {
        check_node(file, n, node, &mut ids)?;
    }
    let starts = journey
        .nodes
        .iter()
        .filter(|node| node.node_type == NodeType::Start)
        .count();
    if starts > 1 {
        return Err(invalid(file, "nodes", "has more than one Start node"));
    }
    let kinds: HashMap<&str, NodeType> = journey
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node.node_type))
        .collect();
    check_edges(file, journey, &kinds)?;
    check_acyclic(file, journey, &kinds)
}

#[cfg(test)]
pub mod tests_support {
    use super::*;

    pub fn edge(id: &str, from: &str, to: &str, when: &str) -> JourneyEdge {
        JourneyEdge {
            id: id.into(),
            from: from.into(),
            to: to.into(),
            when: when.into(),
            map: Vec::new(),
        }
    }

    pub fn request(id: &str, path: &str) -> JourneyNode {
        JourneyNode {
            request: path.into(),
            ..JourneyNode::new(id, NodeType::Request)
        }
    }

    /// Start, then a request, then End.
    pub fn sample() -> Journey {
        Journey {
            name: "Signup".into(),
            nodes: vec![
                JourneyNode::new("s", NodeType::Start),
                request("r", "auth/login.http.yaml"),
                JourneyNode::new("e", NodeType::End),
            ],
            edges: vec![edge("e1", "s", "r", ""), edge("e2", "r", "e", "")],
            settings: JourneySettings::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::*;
    use super::*;

    #[test]
    fn a_plain_journey_is_valid() {
        validate("t", &sample()).unwrap();
    }

    #[test]
    fn rejects_bad_ids_paths_and_dangling_edges() {
        let mut journey = sample();
        journey.nodes[2].id = "r".into();
        assert!(validate("t", &journey).is_err());
        let mut journey = sample();
        journey.nodes[1].request = "../secret.http.yaml".into();
        assert!(validate("t", &journey).is_err());
        let mut journey = sample();
        journey.edges.push(edge("e3", "r", "nope", ""));
        assert!(validate("t", &journey).is_err());
        let mut journey = sample();
        journey.edges[1].when = "true".into();
        assert!(validate("t", &journey).is_err());
    }

    #[test]
    fn a_cycle_must_pass_through_a_loop() {
        let mut journey = sample();
        journey.edges.push(edge("e3", "r", "r2", ""));
        journey.nodes.push(request("r2", "a.http.yaml"));
        journey.edges.push(edge("e4", "r2", "r", ""));
        assert!(validate("t", &journey).is_err());

        let mut looped = sample();
        looped.nodes[1] = JourneyNode::new("r", NodeType::Loop);
        looped.nodes.push(request("body", "a.http.yaml"));
        looped.edges = vec![
            edge("e1", "s", "r", ""),
            edge("e2", "r", "body", WHEN_EACH),
            edge("e3", "body", "r", ""),
            edge("e4", "r", "e", WHEN_DONE),
        ];
        validate("t", &looped).unwrap();
    }

    #[test]
    fn limits_are_enforced() {
        let mut journey = sample();
        journey.settings.max_steps = 0;
        assert!(validate("t", &journey).is_err());
        let mut journey = sample();
        journey.nodes.push(JourneyNode::new("s2", NodeType::Start));
        assert!(validate("t", &journey).is_err());
        let mut journey = sample();
        journey.nodes[1].outputs = vec![Output {
            name: "bad name".into(),
            ..Output::default()
        }];
        assert!(validate("t", &journey).is_err());
    }

    #[test]
    fn edge_mappings_are_checked_like_outputs() {
        let mut journey = sample();
        journey.edges[1].map = vec![Output {
            name: "token".into(),
            source: CheckSource::Body,
            path: "data.token".into(),
        }];
        validate("t", &journey).unwrap();
        journey.edges[1].map[0].name = "bad name".into();
        assert!(validate("t", &journey).is_err());
        journey.edges[1].map[0].name = "token".into();
        journey.edges[1].map[0].source = CheckSource::Variable;
        assert!(validate("t", &journey).is_err());
        journey.edges[1].map = vec![Output::default(); MAX_OUTPUTS + 1];
        assert!(validate("t", &journey).is_err());
    }

    #[test]
    fn a_loop_data_file_stays_inside_the_collection() {
        let mut journey = sample();
        journey.nodes[1] = JourneyNode::new("r", NodeType::Loop);
        journey.edges[1].when = WHEN_DONE.into();
        for good in ["data/users.csv", "users.JSON"] {
            journey.nodes[1].data_file = good.into();
            validate("t", &journey).unwrap();
        }
        for bad in [
            "../users.csv",
            "/etc/users.csv",
            "a\\b.csv",
            "users.txt",
            "./u.csv",
        ] {
            journey.nodes[1].data_file = bad.into();
            assert!(validate("t", &journey).is_err(), "{bad}");
        }
        journey.nodes[1].data_file = "users.csv".into();
        journey.nodes[1].items = vec!["a".into()];
        assert!(validate("t", &journey).is_err());
    }
}
