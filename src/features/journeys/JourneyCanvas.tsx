import {
  Background,
  Controls,
  MiniMap,
  ReactFlow,
  ReactFlowProvider,
  useReactFlow,
  type Connection,
  type Edge,
  type NodeChange,
  type NodeTypes,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { useMemo, type DragEvent } from "react";
import { dropRequestPath, toFlow, type StepNode } from "./flowMap";
import {
  addNode,
  connect,
  moveNode,
  removeEdge,
  removeNode,
  type Journey,
} from "./journeyModel";
import type { RunState } from "./runState";
import { StepCard } from "./StepCard";

const NODE_TYPES: NodeTypes = { step: StepCard };
const SNAP: [number, number] = [20, 20];

export interface JourneyCanvasProps {
  readonly journey: Journey;
  readonly run: RunState;
  readonly onChange: (journey: Journey) => void;
  readonly onSelect: (nodeId: string | null) => void;
}

function Canvas({ journey, run, onChange, onSelect }: JourneyCanvasProps) {
  const flow = useReactFlow();
  const { nodes, edges } = useMemo(() => toFlow(journey, run), [journey, run]);

  const onNodesChange = (changes: NodeChange<StepNode>[]) => {
    let next = journey;
    for (const change of changes) {
      if (change.type === "position" && change.position)
        next = moveNode(next, change.id, change.position.x, change.position.y);
    }
    if (next !== journey) onChange(next);
  };

  const onNodesDelete = (gone: StepNode[]) => {
    onChange(gone.reduce((j, n) => removeNode(j, n.id), journey));
    onSelect(null);
  };

  const onEdgesDelete = (gone: Edge[]) =>
    onChange(gone.reduce((j, e) => removeEdge(j, e.id), journey));

  const onConnect = (c: Connection) =>
    onChange(connect(journey, c.source, c.target));

  const onDrop = (event: DragEvent) => {
    const path = dropRequestPath(event.dataTransfer.getData("text/plain"));
    if (!path) return;
    event.preventDefault();
    const at = flow.screenToFlowPosition({
      x: event.clientX,
      y: event.clientY,
    });
    onChange(addNode(journey, "request", at, { request: path }));
  };

  return (
    <ReactFlow
      nodes={nodes}
      edges={edges}
      nodeTypes={NODE_TYPES}
      onNodesChange={onNodesChange}
      onNodesDelete={onNodesDelete}
      onEdgesDelete={onEdgesDelete}
      onConnect={onConnect}
      onNodeClick={(_, node) => onSelect(node.id)}
      onPaneClick={() => onSelect(null)}
      onDrop={onDrop}
      onDragOver={(e) => e.preventDefault()}
      snapToGrid
      snapGrid={SNAP}
      fitView
      proOptions={{ hideAttribution: true }}
    >
      <Background />
      <Controls showInteractive={false} />
      <MiniMap pannable zoomable />
    </ReactFlow>
  );
}

/** The React Flow canvas; a controlled view of `journey`. */
export function JourneyCanvas(props: JourneyCanvasProps) {
  return (
    <div className="journey-canvas" aria-label="Journey canvas">
      <ReactFlowProvider>
        <Canvas {...props} />
      </ReactFlowProvider>
    </div>
  );
}
