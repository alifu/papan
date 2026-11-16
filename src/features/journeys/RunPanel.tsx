import type { JourneyNode } from "../../ipc/bindings";
import { nodeResult, type RunState } from "./runState";

const FINISHED_TEXT = {
  passed: "Run passed",
  failed: "Run failed",
  cancelled: "Run cancelled",
} as const;

export interface RunPanelProps {
  readonly run: RunState;
  readonly node: JourneyNode | null;
}

/** The outcome of the run, and what the selected node did in it. */
export function RunPanel({ run, node }: RunPanelProps) {
  const result = node ? nodeResult(run, node.id) : undefined;
  return (
    <section className="journey-run" aria-label="Run result">
      <p className="journey-run-line" role="status">
        {run.running
          ? `Running… ${run.steps} steps`
          : run.finished
            ? `${FINISHED_TEXT[run.finished]} (${run.steps} steps)${run.message ? `: ${run.message}` : ""}`
            : "Not run yet."}
      </p>
      {node && result ? (
        <div className="journey-run-node">
          <h3>{node.label || node.id}</h3>
          {result.message ? <p>{result.message}</p> : null}
          {result.response ? (
            <>
              <p>
                Response <strong>{result.response.status}</strong>
              </p>
              <pre aria-label="Response body" className="journey-body">
                {result.response.body_preview}
                {result.response.body_truncated ? "\n… (cut)" : ""}
              </pre>
            </>
          ) : null}
          {result.vars.length > 0 ? (
            <ul aria-label="Variables set">
              {result.vars.map((v) => (
                <li key={v.name}>
                  <code>{v.name}</code> = <code>{v.value}</code>
                </li>
              ))}
            </ul>
          ) : null}
          {result.console.length > 0 ? (
            <pre aria-label="Console" className="journey-body">
              {result.console.join("\n")}
            </pre>
          ) : null}
        </div>
      ) : node && run.finished ? (
        <p className="empty-text">This node did not run.</p>
      ) : null}
    </section>
  );
}
