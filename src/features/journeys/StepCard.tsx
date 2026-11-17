import { Handle, Position, type NodeProps } from "@xyflow/react";
import { nodeSummary, type StepNode } from "./flowMap";
import { NODE_LABELS } from "./journeyModel";

const STATUS_TEXT = {
  running: "Running",
  passed: "Passed",
  failed: "Failed",
} as const;

/** One node of the canvas: type, title, a one-line summary and the live status. */
export function StepCard({ data, selected }: NodeProps<StepNode>) {
  const { node, status, runs } = data;
  const classes = [
    "journey-node",
    `journey-node-${node.type}`,
    selected ? "journey-node-selected" : "",
    status ? `journey-node-${status}` : "",
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <div className={classes}>
      {node.type === "start" ? null : (
        <Handle type="target" position={Position.Left} />
      )}
      <div className="journey-node-head">
        <span className="journey-node-type">{NODE_LABELS[node.type]}</span>
        {status ? (
          <span
            className={`journey-node-status journey-node-status-${status}`}
            role="img"
            aria-label={STATUS_TEXT[status]}
          />
        ) : null}
        {runs > 1 ? <span className="journey-node-runs">×{runs}</span> : null}
      </div>
      <div className="journey-node-title">
        {node.label || nodeSummary(node)}
      </div>
      {node.label ? (
        <div className="journey-node-sub">{nodeSummary(node)}</div>
      ) : null}
      {node.type === "end" ? null : (
        <Handle type="source" position={Position.Right} />
      )}
    </div>
  );
}
