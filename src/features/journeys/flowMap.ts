import type { Edge, Node } from "@xyflow/react";
import type { JourneyEdge, JourneyNode, TreeNode } from "../../ipc/bindings";
import type { Journey } from "./journeyModel";
import type { NodeRunStatus, RunState } from "./runState";

/** What the node card needs. */
export interface StepData extends Record<string, unknown> {
  readonly node: JourneyNode;
  readonly status: NodeRunStatus | null;
  readonly runs: number;
}

export type StepNode = Node<StepData, "step">;

const SPREAD_X = 220;

export function nodeSummary(node: JourneyNode): string {
  switch (node.type) {
    case "request":
      return node.request || "Choose a request";
    case "condition":
    case "assert": {
      const c = node.check;
      return [c?.source ?? "status", c?.name, c?.op ?? "eq", c?.value]
        .filter(Boolean)
        .join(" ");
    }
    case "delay":
      return `${node.delay_ms ?? 0} ms`;
    case "loop": {
      const as = node.loop_var || "item";
      const items = node.items ?? [];
      if (node.data_file) return `rows of ${node.data_file}`;
      return items.length > 0
        ? `${items.length} items as ${as}`
        : `${node.count ?? 0} times as ${as}`;
    }
    case "script":
      return (node.script ?? "").split("\n")[0] || "Empty script";
    default:
      return "";
  }
}

/** Branch name, then the variables the edge sets (`each ⇢ auth`). */
export function edgeLabel(e: JourneyEdge): string | undefined {
  const names = (e.map ?? []).map((m) => m.name).filter(Boolean);
  const mapped = names.length > 0 ? `⇢ ${names.join(", ")}` : "";
  return [e.when, mapped].filter(Boolean).join(" ") || undefined;
}

/** What is picked on the canvas, so the nodes and edges can be drawn selected. */
export interface PickedItem {
  readonly kind: "node" | "edge";
  readonly id: string;
}

export type NodeSizes = Readonly<
  Record<string, { readonly width: number; readonly height: number }>
>;

export function toFlow(
  journey: Journey,
  run: RunState,
  picked: PickedItem | null = null,
  sizes: NodeSizes = {},
): { nodes: StepNode[]; edges: Edge[] } {
  const nodes = (journey.nodes ?? []).map((node, index): StepNode => ({
    id: node.id,
    type: "step",
    position: { x: node.x ?? index * SPREAD_X, y: node.y ?? 0 },
    deletable: node.type !== "start",
    selected: picked?.kind === "node" && picked.id === node.id,
    ...(sizes[node.id] ? { measured: sizes[node.id] } : {}),
    data: {
      node,
      status: run.status[node.id] ?? null,
      runs: run.runs[node.id] ?? 0,
    },
  }));
  const edges = (journey.edges ?? []).map((e): Edge => ({
    id: e.id,
    source: e.from,
    target: e.to,
    label: edgeLabel(e),
    selected: picked?.kind === "edge" && picked.id === e.id,
  }));
  return { nodes, edges };
}

/** Every request of a tree, in order, folders flattened. */
export function flattenRequests(children: readonly TreeNode[]): TreeNode[] {
  return children.flatMap((child) =>
    child.kind === "folder"
      ? flattenRequests(child.children)
      : child.kind === "request" &&
          child.request_kind !== "ws" &&
          child.request_kind !== "grpc"
        ? [child]
        : [],
  );
}

const REQUEST_FILE = /\.[a-z]+\.yaml$/;

/** The sidebar tree drags a path as plain text; only a request file counts. */
export const dropRequestPath = (text: string): string | null =>
  REQUEST_FILE.test(text.trim()) ? text.trim() : null;
