import { useEffect, useState } from "react";
import { useTabsStore } from "../../shell/tabsStore";
import { Button } from "../../ui/Button";
import { reportError, showToast } from "../../ui/toastStore";
import { useCollectionsStore } from "../collections/collectionsStore";
import * as api from "./api";
import { EdgeInspector } from "./EdgeInspector";
import { flattenRequests } from "./flowMap";
import { JourneyCanvas } from "./JourneyCanvas";
import {
  addNode,
  autoLayout,
  draftProblems,
  NODE_LABELS,
  PALETTE,
  patchEdge,
  patchNode,
  removeEdge,
  removeNode,
  type Journey,
  type NodeType,
} from "./journeyModel";
import { journeyKey, useJourneyStore } from "./journeyStore";
import { NodeInspector } from "./NodeInspector";
import { EMPTY_RUN } from "./runState";
import { RunPanel } from "./RunPanel";
import "./journeys.css";

type Selection = { kind: "node" | "edge"; id: string };

const NEW_NODE_STEP = 40;
const NEW_NODE_ORIGIN = { x: 220, y: 120 };

export interface JourneyTabProps {
  readonly collectionId: string;
  readonly file: string;
  readonly tabId: string;
}

export function JourneyTab({ collectionId, file, tabId }: JourneyTabProps) {
  const [draft, setDraft] = useState<Journey | null>(null);
  const [saved, setSaved] = useState<Journey | null>(null);
  const [selection, setSelection] = useState<Selection | null>(null);
  const run =
    useJourneyStore((s) => s.runs[journeyKey(collectionId, file)]) ?? EMPTY_RUN;
  const tree = useCollectionsStore((s) =>
    s.trees.find((t) => t.collection.id === collectionId),
  );

  useEffect(() => {
    let current = true;
    api
      .readJourney(collectionId, file)
      .then((journey) => {
        if (!current) return;
        setDraft(journey);
        setSaved(journey);
      })
      .catch(reportError);
    return () => {
      current = false;
    };
  }, [collectionId, file]);

  if (!draft) return <p className="empty-text">Loading…</p>;
  const dirty = JSON.stringify(draft) !== JSON.stringify(saved);
  const problems = draftProblems(draft);
  const node =
    selection?.kind === "node"
      ? (draft.nodes?.find((n) => n.id === selection.id) ?? null)
      : null;
  const edge =
    selection?.kind === "edge"
      ? (draft.edges?.find((e) => e.id === selection.id) ?? null)
      : null;
  const titleOf = (id: string): string => {
    const n = draft.nodes?.find((m) => m.id === id);
    return n ? n.label || NODE_LABELS[n.type] : id;
  };
  const requests = flattenRequests(tree?.children ?? []);

  const save = async (): Promise<boolean> => {
    try {
      await api.saveJourney(collectionId, file, draft);
      setSaved(draft);
      useTabsStore.getState().retitleTab(tabId, draft.name);
      await useJourneyStore.getState().load(collectionId);
      return true;
    } catch (err) {
      reportError(err);
      return false;
    }
  };

  const removeSelected = () => {
    if (!selection) return;
    setDraft(
      selection.kind === "node"
        ? removeNode(draft, selection.id)
        : removeEdge(draft, selection.id),
    );
    setSelection(null);
  };

  const toggleRun = async () => {
    const store = useJourneyStore.getState();
    if (run.running) return store.cancel(collectionId, file);
    if (dirty && !(await save())) return;
    if (problems.length > 0)
      showToast({ kind: "info", message: problems.join(" ") });
    await store.run(collectionId, file);
  };

  const add = (type: NodeType) => {
    const count = draft.nodes?.length ?? 0;
    const offset = (count % 8) * NEW_NODE_STEP;
    setDraft(
      addNode(draft, type, {
        x: NEW_NODE_ORIGIN.x + offset,
        y: NEW_NODE_ORIGIN.y + offset,
      }),
    );
  };

  return (
    <div className="journey-tab">
      <header className="journey-head">
        <input
          aria-label="Journey name"
          className="journey-name"
          value={draft.name}
          onChange={(e) => setDraft({ ...draft, name: e.target.value })}
        />
        <label className="journey-policy">
          On failure
          <select
            value={draft.settings?.on_failure ?? "stop"}
            onChange={(e) =>
              setDraft({
                ...draft,
                settings: {
                  ...draft.settings,
                  on_failure:
                    e.target.value === "continue" ? "continue" : "stop",
                },
              })
            }
          >
            <option value="stop">Stop the run</option>
            <option value="continue">Continue</option>
          </select>
        </label>
        <Button label="Tidy up" onClick={() => setDraft(autoLayout(draft))} />
        <Button label="Save" disabled={!dirty} onClick={() => void save()} />
        <Button
          icon={run.running ? "stop" : "play"}
          label={run.running ? "Stop" : "Run"}
          onClick={() => void toggleRun()}
        />
        {dirty ? <span className="journey-dirty">Unsaved changes</span> : null}
      </header>
      {problems.length > 0 ? (
        <p className="journey-problems" role="status">
          {problems.join(" ")}
        </p>
      ) : null}
      <div className="journey-body-row">
        <nav className="journey-palette" aria-label="Add a node">
          {PALETTE.map((type) => (
            <Button
              key={type}
              icon="plus"
              label={NODE_LABELS[type]}
              aria-label={`Add ${NODE_LABELS[type]} node`}
              onClick={() => add(type)}
            />
          ))}
        </nav>
        <JourneyCanvas
          journey={draft}
          run={run}
          picked={selection}
          onChange={setDraft}
          onSelect={(id) => setSelection(id ? { kind: "node", id } : null)}
          onSelectEdge={(id) => setSelection({ kind: "edge", id })}
        />
        <aside className="journey-side">
          {edge ? (
            <EdgeInspector
              edge={edge}
              fromLabel={titleOf(edge.from)}
              toLabel={titleOf(edge.to)}
              onChange={(patch) => setDraft(patchEdge(draft, edge.id, patch))}
            />
          ) : node ? (
            <NodeInspector
              node={node}
              requests={requests}
              onChange={(patch) => setDraft(patchNode(draft, node.id, patch))}
            />
          ) : (
            <p className="empty-text">Select a node to edit it.</p>
          )}
          {edge || (node && node.type !== "start") ? (
            <Button
              icon="close"
              label={edge ? "Delete edge" : "Delete node"}
              onClick={removeSelected}
            />
          ) : null}
          <RunPanel run={run} node={node} />
        </aside>
      </div>
    </div>
  );
}
