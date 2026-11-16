import { describe, expect, it } from "vitest";
import {
  addNode,
  branchLabels,
  connect,
  draftProblems,
  moveNode,
  newJourney,
  patchNode,
  removeEdge,
  removeNode,
  requestPathOf,
} from "./journeyModel";

const withRequest = () => {
  const j0 = newJourney("Flow");
  const j1 = addNode(j0, "request", { x: 200, y: 0 });
  const j2 = addNode(j1, "end", { x: 400, y: 0 });
  return {
    j: j2,
    start: j2.nodes![0]!.id,
    req: j2.nodes![1]!.id,
    end: j2.nodes![2]!.id,
  };
};

describe("journeyModel", () => {
  it("starts a journey with one Start node", () => {
    const j = newJourney("Flow");
    expect(j.name).toBe("Flow");
    expect(j.nodes?.map((n) => n.type)).toEqual(["start"]);
  });

  it("adds nodes with fresh ids and type defaults, without changing the old journey", () => {
    const j0 = newJourney("Flow");
    const j1 = addNode(j0, "loop", { x: 10, y: 20 });
    const j2 = addNode(j1, "loop", { x: 30, y: 40 });
    expect(j0.nodes).toHaveLength(1);
    const ids = j2.nodes!.map((n) => n.id);
    expect(new Set(ids).size).toBe(3);
    expect(j2.nodes![1]).toMatchObject({
      type: "loop",
      x: 10,
      y: 20,
      count: 3,
    });
  });

  it("refuses a second Start node", () => {
    const j = addNode(newJourney("Flow"), "start", { x: 0, y: 0 });
    expect(j.nodes).toHaveLength(1);
  });

  it("removes a node together with its edges", () => {
    const { j, start, req, end } = withRequest();
    const wired = connect(connect(j, start, req), req, end);
    const cut = removeNode(wired, req);
    expect(cut.nodes!.map((n) => n.id)).toEqual([start, end]);
    expect(cut.edges).toEqual([]);
  });

  it("moves and patches a node immutably", () => {
    const { j, req } = withRequest();
    const moved = moveNode(j, req, 5, 6);
    expect(moved.nodes!.find((n) => n.id === req)).toMatchObject({
      x: 5,
      y: 6,
    });
    expect(j.nodes!.find((n) => n.id === req)).toMatchObject({ x: 200 });
    const patched = patchNode(j, req, { request: "a/b.http.yaml" });
    expect(requestPathOf(patched, req)).toBe("a/b.http.yaml");
    expect(requestPathOf(j, req)).toBe("");
  });

  it("connects nodes once and refuses nonsense edges", () => {
    const { j, start, req, end } = withRequest();
    const one = connect(j, start, req);
    expect(one.edges).toHaveLength(1);
    expect(connect(one, start, req).edges).toHaveLength(1); // duplicate
    expect(connect(j, req, req).edges ?? []).toHaveLength(0); // self
    expect(connect(j, end, req).edges ?? []).toHaveLength(0); // nothing leaves End
    expect(connect(j, req, start).edges ?? []).toHaveLength(0); // nothing enters Start
    expect(connect(j, "ghost", req).edges ?? []).toHaveLength(0);
  });

  it("labels the branches of a Condition and a Loop in turn", () => {
    expect(branchLabels("condition")).toEqual(["true", "false"]);
    expect(branchLabels("loop")).toEqual(["each", "done"]);
    expect(branchLabels("request")).toEqual([]);
    let j = addNode(newJourney("Flow"), "condition", { x: 0, y: 0 });
    j = addNode(j, "end", { x: 0, y: 0 });
    j = addNode(j, "end", { x: 0, y: 0 });
    j = addNode(j, "end", { x: 0, y: 0 });
    const [, cond, a, b, c] = j.nodes!.map((n) => n.id) as [
      string,
      string,
      string,
      string,
      string,
    ];
    j = connect(j, cond, a);
    j = connect(j, cond, b);
    expect(j.edges!.map((e) => e.when)).toEqual(["true", "false"]);
    expect(connect(j, cond, c).edges).toHaveLength(2); // both branches used
  });

  it("removes an edge by id", () => {
    const { j, start, req } = withRequest();
    const one = connect(j, start, req);
    expect(removeEdge(one, one.edges![0]!.id).edges).toEqual([]);
  });

  it("lists what would stop a run", () => {
    const { j, start, req } = withRequest();
    expect(draftProblems(j).join(" ")).toMatch(/not reached/i);
    const wired = connect(j, start, req);
    expect(draftProblems(wired).join(" ")).toMatch(/no request/i);
    const ready = patchNode(wired, req, { request: "a.http.yaml" });
    expect(draftProblems(ready).join(" ")).not.toMatch(/no request/i);
    const noStart = removeNode(j, start);
    expect(draftProblems(noStart).join(" ")).toMatch(/start/i);
  });
});
