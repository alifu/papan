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
import { useEffect, useMemo, useRef, type DragEvent } from "react";
import {
  dropRequestPath,
  toFlow,
  type NodeSizes,
  type PickedItem,
  type StepNode,
} from "./flowMap";
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
const MINIMAP_SIZE = { width: 140, height: 90 };
const FIT = { padding: 0.3, duration: 200 };
const DELETE_KEYS = ["Backspace", "Delete"];

export interface JourneyCanvasProps {
  readonly journey: Journey;
  readonly run: RunState;
  readonly picked: PickedItem | null;
  readonly onChange: (journey: Journey) => void;
  readonly onSelect: (nodeId: string | null) => void;
  readonly onSelectEdge: (edgeId: string) => void;
}

function Canvas({
  journey,
  run,
  picked,
  onChange,
  onSelect,
  onSelectEdge,
}: JourneyCanvasProps) {
  const flow = useReactFlow();
  // React Flow measures the cards itself; hand the sizes back so a rebuild keeps them.
  const sizes = useRef<NodeSizes>({});
  const { nodes, edges } = useMemo(
    () => toFlow(journey, run, picked, sizes.current),
    [journey, run, picked],
  );

  // A node added from the palette lands where the view may not reach: show everything again.
  const count = journey.nodes?.length ?? 0;
  const shown = useRef(count);
  useEffect(() => {
    if (count > shown.current) void flow.fitView(FIT);
    shown.current = count;
  }, [count, flow]);

  const onNodesChange = (changes: NodeChange<StepNode>[]) => {
    let next = journey;
    for (const change of changes) {
      if (change.type === "dimensions" && change.dimensions)
        sizes.current = { ...sizes.current, [change.id]: change.dimensions };
      if (change.type === "position" && change.position)
        next = moveNode(next, change.id, change.position.x, change.position.y);
    }
    if (next !== journey) onChange(next);
  };

  // One handler for both: deleting a node also deletes its edges, and two handlers
  // would each start from the same stale journey and bring the node back.
  const onDelete = ({
    nodes: goneNodes,
    edges: goneEdges,
  }: {
    nodes: StepNode[];
    edges: Edge[];
  }) => {
    const withoutNodes = goneNodes.reduce(
      (j, n) => removeNode(j, n.id),
      journey,
    );
    onChange(goneEdges.reduce((j, e) => removeEdge(j, e.id), withoutNodes));
    onSelect(null);
  };

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
      onDelete={onDelete}
      deleteKeyCode={DELETE_KEYS}
      onConnect={onConnect}
      onNodeClick={(_, node) => onSelect(node.id)}
      onEdgeClick={(_, edge) => onSelectEdge(edge.id)}
      onPaneClick={() => onSelect(null)}
      onDrop={onDrop}
      onDragOver={(e) => e.preventDefault()}
      snapToGrid
      snapGrid={SNAP}
      fitView
      fitViewOptions={FIT}
      proOptions={{ hideAttribution: true }}
    >
      <Background />
      <Controls showInteractive={false} />
      <MiniMap pannable zoomable position="top-right" style={MINIMAP_SIZE} />
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
