import { describe, expect, it } from "vitest";
import type { TreeNode } from "../../ipc/bindings";
import { addNode, connect, newJourney, patchNode } from "./journeyModel";
import {
  dropRequestPath,
  edgeLabel,
  flattenRequests,
  nodeSummary,
  toFlow,
} from "./flowMap";
import { EMPTY_RUN } from "./runState";

describe("flowMap", () => {
  it("maps nodes and edges, with run status and the edge label", () => {
    let j = newJourney("F");
    j = addNode(j, "condition", { x: 220, y: 0 });
    j = connect(j, j.nodes![0]!.id, j.nodes![1]!.id);
    const run = { ...EMPTY_RUN, status: { n2: "failed" as const } };
    const { nodes, edges } = toFlow(j, run);
    expect(nodes.map((n) => [n.id, n.position])).toEqual([
      ["n1", { x: 0, y: 0 }],
      ["n2", { x: 220, y: 0 }],
    ]);
    expect(nodes[1]!.data.status).toBe("failed");
    expect(nodes[0]!.deletable).toBe(false);
    expect(edges).toHaveLength(1);
    expect(edges[0]).toMatchObject({ source: "n1", target: "n2" });
  });

  it("labels branch edges and spreads nodes that have no position", () => {
    let j = newJourney("F");
    j = addNode(j, "loop", { x: 0, y: 0 });
    j = addNode(j, "end", { x: 0, y: 0 }, { x: null, y: null });
    j = connect(j, "n2", "n3");
    const { nodes, edges } = toFlow(j, EMPTY_RUN);
    expect(edges[0]!.label).toBe("each");
    expect(nodes[2]!.position.x).toBeGreaterThan(0);
  });

  it("summarises a node in one line", () => {
    let j = newJourney("F");
    j = addNode(j, "request", { x: 0, y: 0 });
    expect(nodeSummary(j.nodes![1]!)).toMatch(/choose/i);
    j = patchNode(j, "n2", { request: "auth/login.http.yaml" });
    expect(nodeSummary(j.nodes![1]!)).toBe("auth/login.http.yaml");
    j = addNode(j, "assert", { x: 0, y: 0 });
    expect(nodeSummary(j.nodes![2]!)).toBe("status eq 200");
    j = addNode(j, "delay", { x: 0, y: 0 });
    expect(nodeSummary(j.nodes![3]!)).toBe("500 ms");
    j = addNode(j, "loop", { x: 0, y: 0 });
    expect(nodeSummary(j.nodes![4]!)).toBe("3 times as item");
  });

  it("lists the requests of a tree, folders flattened", () => {
    const tree: TreeNode[] = [
      {
        path: "a",
        name: "a",
        kind: "folder",
        method: null,
        request_kind: null,
        children: [
          {
            path: "a/b.http.yaml",
            name: "b",
            kind: "request",
            method: "GET",
            request_kind: "http",
            children: [],
          },
          {
            path: "a/c.ws.yaml",
            name: "c",
            kind: "request",
            method: null,
            request_kind: "ws",
            children: [],
          },
        ],
      },
      {
        path: "d.http.yaml",
        name: "d",
        kind: "request",
        method: "POST",
        request_kind: "graphql",
        children: [],
      },
    ];
    expect(flattenRequests(tree).map((r) => r.path)).toEqual([
      "a/b.http.yaml",
      "d.http.yaml",
    ]);
  });

  it("reads a dropped request path, ignoring anything else", () => {
    expect(dropRequestPath("a/b.http.yaml")).toBe("a/b.http.yaml");
    expect(dropRequestPath("folder")).toBeNull();
    expect(dropRequestPath("")).toBeNull();
  });

  it("labels an edge with its branch and the variables it sets", () => {
    expect(edgeLabel({ id: "e", from: "a", to: "b" })).toBeUndefined();
    expect(edgeLabel({ id: "e", from: "a", to: "b", when: "each" })).toBe(
      "each",
    );
    expect(
      edgeLabel({
        id: "e",
        from: "a",
        to: "b",
        when: "true",
        map: [{ name: "auth" }, { name: "id" }],
      }),
    ).toBe("true ⇢ auth, id");
  });

  it("summarises a loop over a data file", () => {
    let j = newJourney("F");
    j = addNode(j, "loop", { x: 0, y: 0 }, { data_file: "data/u.csv" });
    expect(nodeSummary(j.nodes![1]!)).toBe("rows of data/u.csv");
  });
});

describe("toFlow selection and sizes", () => {
  const j = connect(
    addNode(newJourney("J"), "end", { x: 0, y: 0 }),
    "n1",
    "n2",
  );

  it("marks the picked node or edge as selected", () => {
    const node = toFlow(j, EMPTY_RUN, { kind: "node", id: "n2" });
    expect(node.nodes.map((n) => n.selected)).toEqual([false, true]);
    const edge = toFlow(j, EMPTY_RUN, { kind: "edge", id: j.edges![0]!.id });
    expect(edge.edges[0]!.selected).toBe(true);
    expect(edge.nodes.some((n) => n.selected)).toBe(false);
  });

  it("passes the measured sizes back so a rebuild keeps them", () => {
    const { nodes } = toFlow(j, EMPTY_RUN, null, {
      n1: { width: 180, height: 60 },
    });
    expect(nodes[0]!.measured).toEqual({ width: 180, height: 60 });
    expect(nodes[1]!.measured).toBeUndefined();
  });
});
