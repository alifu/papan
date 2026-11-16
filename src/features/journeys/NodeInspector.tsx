import type {
  Check,
  CheckOp,
  CheckSource,
  FailurePolicy,
  JourneyNode,
  Output,
  TreeNode,
} from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { removeAt, replaceAt } from "../mocks/mockModel";
import { NODE_LABELS } from "./journeyModel";

const SOURCES: readonly CheckSource[] = [
  "status",
  "header",
  "body",
  "variable",
];
const OPS: readonly CheckOp[] = [
  "eq",
  "ne",
  "gt",
  "ge",
  "lt",
  "le",
  "contains",
  "exists",
  "not_exists",
];
const NEEDS_NAME: readonly CheckSource[] = ["header", "body", "variable"];
const OUTPUT_SOURCES: readonly CheckSource[] = ["body", "header", "status"];

export interface NodeInspectorProps {
  readonly node: JourneyNode;
  readonly requests: readonly TreeNode[];
  readonly onChange: (patch: Partial<JourneyNode>) => void;
}

function CheckFields({
  check,
  onChange,
}: {
  readonly check: Check;
  readonly onChange: (check: Check) => void;
}) {
  const source = check.source ?? "status";
  return (
    <fieldset>
      <legend>Check</legend>
      <label>
        Source
        <select
          value={source}
          onChange={(e) =>
            onChange({ ...check, source: e.target.value as CheckSource })
          }
        >
          {SOURCES.map((s) => (
            <option key={s}>{s}</option>
          ))}
        </select>
      </label>
      {NEEDS_NAME.includes(source) ? (
        <label>
          {source === "body" ? "JSON path" : "Name"}
          <input
            value={check.name ?? ""}
            onChange={(e) => onChange({ ...check, name: e.target.value })}
          />
        </label>
      ) : null}
      <label>
        Operator
        <select
          value={check.op ?? "eq"}
          onChange={(e) =>
            onChange({ ...check, op: e.target.value as CheckOp })
          }
        >
          {OPS.map((o) => (
            <option key={o}>{o}</option>
          ))}
        </select>
      </label>
      <label>
        Value
        <input
          value={check.value ?? ""}
          onChange={(e) => onChange({ ...check, value: e.target.value })}
        />
      </label>
    </fieldset>
  );
}

function OutputsEditor({
  outputs,
  onChange,
}: {
  readonly outputs: readonly Output[];
  readonly onChange: (outputs: Output[]) => void;
}) {
  const set = (i: number, patch: Partial<Output>) =>
    onChange([...replaceAt(outputs, i, { ...outputs[i]!, ...patch })]);
  return (
    <fieldset>
      <legend>Keep from the response</legend>
      {outputs.map((o, i) => (
        <div key={i} className="journey-row">
          <input
            aria-label={`Output ${i + 1} name`}
            placeholder="variable"
            value={o.name}
            onChange={(e) => set(i, { name: e.target.value })}
          />
          <select
            aria-label={`Output ${i + 1} source`}
            value={o.source ?? "body"}
            onChange={(e) => set(i, { source: e.target.value as CheckSource })}
          >
            {OUTPUT_SOURCES.map((s) => (
              <option key={s}>{s}</option>
            ))}
          </select>
          {(o.source ?? "body") === "status" ? null : (
            <input
              aria-label={`Output ${i + 1} path`}
              placeholder={
                (o.source ?? "body") === "body" ? "data.token" : "Header"
              }
              value={o.path ?? ""}
              onChange={(e) => set(i, { path: e.target.value })}
            />
          )}
          <Button
            icon="close"
            aria-label={`Remove output ${i + 1}`}
            onClick={() => onChange([...removeAt(outputs, i)])}
          />
        </div>
      ))}
      <Button
        icon="plus"
        label="Add output"
        onClick={() =>
          onChange([...outputs, { name: "", source: "body", path: "" }])
        }
      />
    </fieldset>
  );
}

/** Edits the selected node; the fields depend on its type. */
export function NodeInspector({
  node,
  requests,
  onChange,
}: NodeInspectorProps) {
  const editable = node.type !== "start" && node.type !== "end";
  return (
    <section className="journey-inspector" aria-label="Node settings">
      <h3>{NODE_LABELS[node.type]}</h3>
      <label>
        Label
        <input
          value={node.label ?? ""}
          onChange={(e) => onChange({ label: e.target.value })}
        />
      </label>
      {node.type === "request" ? (
        <>
          <label>
            Request
            <select
              value={node.request ?? ""}
              onChange={(e) => onChange({ request: e.target.value })}
            >
              <option value="">Choose…</option>
              {node.request &&
              !requests.some((r) => r.path === node.request) ? (
                <option value={node.request}>{node.request} (missing)</option>
              ) : null}
              {requests.map((r) => (
                <option key={r.path} value={r.path}>
                  {r.path}
                </option>
              ))}
            </select>
          </label>
          <OutputsEditor
            outputs={node.outputs ?? []}
            onChange={(outputs) => onChange({ outputs })}
          />
        </>
      ) : null}
      {node.type === "script" ? (
        <label>
          Script
          <textarea
            rows={8}
            value={node.script ?? ""}
            onChange={(e) => onChange({ script: e.target.value })}
          />
        </label>
      ) : null}
      {node.type === "condition" || node.type === "assert" ? (
        <CheckFields
          check={node.check ?? {}}
          onChange={(check) => onChange({ check })}
        />
      ) : null}
      {node.type === "delay" ? (
        <label>
          Delay (ms)
          <input
            type="number"
            min={0}
            value={node.delay_ms ?? 0}
            onChange={(e) =>
              onChange({ delay_ms: Math.max(0, Number(e.target.value) || 0) })
            }
          />
        </label>
      ) : null}
      {node.type === "loop" ? (
        <>
          <label>
            Variable
            <input
              value={node.loop_var ?? ""}
              onChange={(e) => onChange({ loop_var: e.target.value })}
            />
          </label>
          <label>
            Times (when there are no items)
            <input
              type="number"
              min={0}
              value={node.count ?? 0}
              onChange={(e) =>
                onChange({ count: Math.max(0, Number(e.target.value) || 0) })
              }
            />
          </label>
          <label>
            Data file (.csv or .json in the collection)
            <input
              value={node.data_file ?? ""}
              placeholder="data/users.csv"
              onChange={(e) => onChange({ data_file: e.target.value })}
            />
          </label>
          <label>
            Items (one per line)
            <textarea
              rows={4}
              disabled={Boolean(node.data_file)}
              value={(node.items ?? []).join("\n")}
              onChange={(e) =>
                onChange({
                  items:
                    e.target.value === "" ? [] : e.target.value.split("\n"),
                })
              }
            />
          </label>
        </>
      ) : null}
      {editable ? (
        <label>
          When this node fails
          <select
            value={node.on_failure ?? ""}
            onChange={(e) =>
              onChange({
                on_failure: (e.target.value || null) as FailurePolicy | null,
              })
            }
          >
            <option value="">Use the journey setting</option>
            <option value="stop">Stop the run</option>
            <option value="continue">Continue</option>
          </select>
        </label>
      ) : null}
    </section>
  );
}
