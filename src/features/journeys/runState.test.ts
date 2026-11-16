import { describe, expect, it } from "vitest";
import type { RunEvent } from "../../ipc/bindings";
import { applyRunEvent, EMPTY_RUN, nodeResult } from "./runState";

const event = (over: Partial<RunEvent>): RunEvent => ({
  kind: "node_started",
  node_id: "n1",
  status: null,
  message: "",
  response: null,
  vars: [],
  console: [],
  steps: 0,
  run_status: null,
  ...over,
});

describe("runState", () => {
  it("marks a node running, then with its outcome", () => {
    const a = applyRunEvent(EMPTY_RUN, event({}));
    expect(a.status.n1).toBe("running");
    expect(a.running).toBe(true);
    const b = applyRunEvent(
      a,
      event({
        kind: "node_finished",
        status: "passed",
        message: "ok",
        vars: [{ name: "token", value: "x" }],
        steps: 1,
      }),
    );
    expect(b.status.n1).toBe("passed");
    expect(nodeResult(b, "n1")?.vars).toEqual([{ name: "token", value: "x" }]);
    expect(b.steps).toBe(1);
  });

  it("keeps the last result of a node that runs again in a loop", () => {
    let s = applyRunEvent(
      EMPTY_RUN,
      event({ kind: "node_finished", status: "failed", message: "one" }),
    );
    s = applyRunEvent(s, event({ kind: "node_started" }));
    expect(s.status.n1).toBe("running");
    s = applyRunEvent(
      s,
      event({ kind: "node_finished", status: "passed", message: "two" }),
    );
    expect(nodeResult(s, "n1")?.message).toBe("two");
    expect(s.runs.n1).toBe(2);
  });

  it("ends the run with its status", () => {
    const s = applyRunEvent(
      EMPTY_RUN,
      event({
        kind: "run_finished",
        node_id: "",
        run_status: "failed",
        message: "stopped",
      }),
    );
    expect(s.running).toBe(false);
    expect(s.finished).toBe("failed");
    expect(s.message).toBe("stopped");
  });
});
