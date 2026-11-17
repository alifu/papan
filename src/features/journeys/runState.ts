import type { RunEvent, RunStatus } from "../../ipc/bindings";

export type NodeRunStatus = "running" | "passed" | "failed";

export interface NodeResult {
  readonly message: string;
  readonly response: RunEvent["response"];
  readonly vars: RunEvent["vars"];
  readonly console: RunEvent["console"];
}

/** What the canvas shows of one run; replaced whole on every event. */
export interface RunState {
  readonly running: boolean;
  readonly status: Readonly<Record<string, NodeRunStatus>>;
  readonly results: Readonly<Record<string, NodeResult>>;
  /** How many times each node has finished (a loop body finishes many times). */
  readonly runs: Readonly<Record<string, number>>;
  readonly steps: number;
  readonly finished: RunStatus | null;
  readonly message: string;
}

export const EMPTY_RUN: RunState = {
  running: false,
  status: {},
  results: {},
  runs: {},
  steps: 0,
  finished: null,
  message: "",
};

export const nodeResult = (
  state: RunState,
  id: string,
): NodeResult | undefined => state.results[id];

export function applyRunEvent(state: RunState, e: RunEvent): RunState {
  if (e.kind === "run_finished")
    return {
      ...state,
      running: false,
      finished: e.run_status,
      message: e.message,
      steps: e.steps || state.steps,
    };
  if (e.kind === "node_started")
    return {
      ...state,
      running: true,
      finished: null,
      status: { ...state.status, [e.node_id]: "running" },
    };
  const status = e.status ?? "passed";
  return {
    ...state,
    running: true,
    status: { ...state.status, [e.node_id]: status },
    results: {
      ...state.results,
      [e.node_id]: {
        message: e.message,
        response: e.response,
        vars: e.vars,
        console: e.console,
      },
    },
    runs: { ...state.runs, [e.node_id]: (state.runs[e.node_id] ?? 0) + 1 },
    steps: e.steps,
  };
}
