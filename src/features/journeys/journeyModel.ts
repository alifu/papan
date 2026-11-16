import type {
  Journey,
  JourneyEdge,
  JourneyNode,
  NodeType,
} from "../../ipc/bindings";

export type { Journey, JourneyEdge, JourneyNode, NodeType };

export const NODE_LABELS: Readonly<Record<NodeType, string>> = {
  start: "Start",
  end: "End",
  request: "Request",
  script: "Script",
  condition: "Condition",
  delay: "Delay",
  assert: "Assert",
  loop: "Loop",
};

/** Node types the palette offers (there is one Start, made with the journey). */
export const PALETTE: readonly NodeType[] = [
  "request",
  "script",
  "condition",
  "assert",
  "delay",
  "loop",
  "end",
];

const DEFAULT_DELAY_MS = 500;
const DEFAULT_LOOP_COUNT = 3;
const DEFAULT_LOOP_VAR = "item";

const nodesOf = (j: Journey): readonly JourneyNode[] => j.nodes ?? [];
const edgesOf = (j: Journey): readonly JourneyEdge[] => j.edges ?? [];

const freshId = (prefix: string, taken: readonly string[]): string => {
  let n = taken.length + 1;
  while (taken.includes(`${prefix}${n}`)) n += 1;
  return `${prefix}${n}`;
};

const defaultsFor = (type: NodeType): Partial<JourneyNode> => {
  switch (type) {
    case "request":
      return { request: "", outputs: [] };
    case "script":
      return { script: "" };
    case "condition":
    case "assert":
      return { check: { source: "status", op: "eq", value: "200" } };
    case "delay":
      return { delay_ms: DEFAULT_DELAY_MS };
    case "loop":
      return { count: DEFAULT_LOOP_COUNT, loop_var: DEFAULT_LOOP_VAR };
    default:
      return {};
  }
};

export const newJourney = (name: string): Journey =>
  addNode({ name, nodes: [], edges: [] }, "start", { x: 0, y: 0 });

export function addNode(
  j: Journey,
  type: NodeType,
  at: { readonly x: number; readonly y: number },
  init: Partial<JourneyNode> = {},
): Journey {
  const nodes = nodesOf(j);
  if (type === "start" && nodes.some((n) => n.type === "start")) return j;
  const id = freshId(
    "n",
    nodes.map((n) => n.id),
  );
  const node: JourneyNode = {
    id,
    type,
    x: at.x,
    y: at.y,
    ...defaultsFor(type),
    ...init,
  };
  return { ...j, nodes: [...nodes, node] };
}

export const removeNode = (j: Journey, id: string): Journey => ({
  ...j,
  nodes: nodesOf(j).filter((n) => n.id !== id),
  edges: edgesOf(j).filter((e) => e.from !== id && e.to !== id),
});

export const patchNode = (
  j: Journey,
  id: string,
  patch: Partial<JourneyNode>,
): Journey => ({
  ...j,
  nodes: nodesOf(j).map((n) => (n.id === id ? { ...n, ...patch } : n)),
});

export const moveNode = (
  j: Journey,
  id: string,
  x: number,
  y: number,
): Journey => patchNode(j, id, { x, y });

export const requestPathOf = (j: Journey, id: string): string =>
  nodesOf(j).find((n) => n.id === id)?.request ?? "";

export function branchLabels(type: NodeType): readonly string[] {
  if (type === "condition") return ["true", "false"];
  if (type === "loop") return ["each", "done"];
  return [];
}

/** Adds an edge unless it makes no sense; a branching node gets its next free label. */
export function connect(j: Journey, from: string, to: string): Journey {
  const nodes = nodesOf(j);
  const edges = edgesOf(j);
  const source = nodes.find((n) => n.id === from);
  const target = nodes.find((n) => n.id === to);
  if (!source || !target || from === to) return j;
  if (source.type === "end" || target.type === "start") return j;
  if (edges.some((e) => e.from === from && e.to === to)) return j;
  const labels = branchLabels(source.type);
  let when = "";
  if (labels.length > 0) {
    const used = edges.filter((e) => e.from === from).map((e) => e.when);
    const free = labels.find((l) => !used.includes(l));
    if (!free) return j;
    when = free;
  }
  const id = freshId(
    "e",
    edges.map((e) => e.id),
  );
  return { ...j, edges: [...edges, { id, from, to, when }] };
}

export const removeEdge = (j: Journey, id: string): Journey => ({
  ...j,
  edges: edgesOf(j).filter((e) => e.id !== id),
});

const reachable = (j: Journey): Set<string> => {
  const start = nodesOf(j).find((n) => n.type === "start");
  const seen = new Set<string>();
  const queue = start ? [start.id] : [];
  while (queue.length > 0) {
    const id = queue.pop()!;
    if (seen.has(id)) continue;
    seen.add(id);
    edgesOf(j)
      .filter((e) => e.from === id)
      .forEach((e) => queue.push(e.to));
  }
  return seen;
};

/** What would stop or surprise a run; the backend still validates on save. */
export function draftProblems(j: Journey): string[] {
  const nodes = nodesOf(j);
  if (!nodes.some((n) => n.type === "start"))
    return ["There is no Start node."];
  const seen = reachable(j);
  const problems: string[] = [];
  const stray = nodes.filter((n) => !seen.has(n.id));
  if (stray.length > 0)
    problems.push(`${stray.length} node(s) are not reached from Start.`);
  const bare = nodes.filter((n) => n.type === "request" && !n.request);
  if (bare.length > 0)
    problems.push(`${bare.length} Request node(s) have no request chosen.`);
  return problems;
}
