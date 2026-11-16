import type { CheckSource, JourneyEdge, Output } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { removeAt, replaceAt } from "../mocks/mockModel";

const SOURCES: readonly CheckSource[] = ["body", "header", "status"];

export interface EdgeInspectorProps {
  readonly edge: JourneyEdge;
  readonly fromLabel: string;
  readonly toLabel: string;
  readonly onChange: (patch: Partial<JourneyEdge>) => void;
}

/** Edits what travels along one edge: values copied into variables for the next node. */
export function EdgeInspector({
  edge,
  fromLabel,
  toLabel,
  onChange,
}: EdgeInspectorProps) {
  const map = edge.map ?? [];
  const set = (i: number, patch: Partial<Output>) =>
    onChange({ map: [...replaceAt(map, i, { ...map[i]!, ...patch })] });
  return (
    <section className="journey-inspector" aria-label="Edge settings">
      <h3>
        {fromLabel} → {toLabel}
      </h3>
      <p className="journey-hint">
        Copy a value from the last response into a variable the next node can
        use.
      </p>
      {map.map((m, i) => (
        <div key={i} className="journey-row">
          <select
            aria-label={`Mapping ${i + 1} source`}
            value={m.source ?? "body"}
            onChange={(e) => set(i, { source: e.target.value as CheckSource })}
          >
            {SOURCES.map((s) => (
              <option key={s}>{s}</option>
            ))}
          </select>
          {(m.source ?? "body") === "status" ? null : (
            <input
              aria-label={`Mapping ${i + 1} path`}
              placeholder={
                (m.source ?? "body") === "body" ? "data.token" : "Header"
              }
              value={m.path ?? ""}
              onChange={(e) => set(i, { path: e.target.value })}
            />
          )}
          <input
            aria-label={`Mapping ${i + 1} variable`}
            placeholder="variable"
            value={m.name}
            onChange={(e) => set(i, { name: e.target.value })}
          />
          <Button
            icon="close"
            aria-label={`Remove mapping ${i + 1}`}
            onClick={() => onChange({ map: [...removeAt(map, i)] })}
          />
        </div>
      ))}
      <Button
        icon="plus"
        label="Add mapping"
        onClick={() =>
          onChange({ map: [...map, { name: "", source: "body", path: "" }] })
        }
      />
    </section>
  );
}
