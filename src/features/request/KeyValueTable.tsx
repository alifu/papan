import "./request.css";
import { useRef } from "react";
import type { EffectiveVariable } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { unresolvedAttrs } from "../environments/unresolvedAttrs";

export interface KeyValueRow {
  readonly id: string;
  readonly name: string;
  readonly value: string;
  readonly enabled: boolean;
}

export interface KeyValueTableProps {
  readonly rows: readonly KeyValueRow[];
  readonly onChange: (rows: readonly KeyValueRow[]) => void;
  readonly label: string;
  readonly variables?: readonly EffectiveVariable[];
}

type Field = "name" | "value";

/** Editable rows with an always-present empty trailing row. */
export function KeyValueTable({ rows, onChange, label, variables }: KeyValueTableProps) {
  const nextId = useRef(0);
  const last = rows[rows.length - 1];
  const needsTrailing = last === undefined || last.name !== "" || last.value !== "";

  const update = (index: number, patch: Partial<KeyValueRow>) =>
    onChange(rows.map((row, i) => (i === index ? { ...row, ...patch } : row)));

  const editTrailing = (field: Field, text: string) => {
    nextId.current += 1;
    onChange([
      ...rows,
      { id: `new-${nextId.current}-${rows.length}`, name: "", value: "", enabled: true, [field]: text },
    ]);
  };

  const remove = (index: number) => onChange(rows.filter((_, i) => i !== index));

  return (
    <table className="kv-table" aria-label={label}>
      <thead>
        <tr>
          <th scope="col"><span className="visually-hidden">Enabled</span></th>
          <th scope="col">Name</th>
          <th scope="col">Value</th>
          <th scope="col"><span className="visually-hidden">Actions</span></th>
        </tr>
      </thead>
      <tbody>
        {rows.map((row, index) => (
          <tr key={row.id}>
            <td>
              <input
                type="checkbox"
                aria-label={`Enable row ${index + 1}`}
                checked={row.enabled}
                onChange={(event) => update(index, { enabled: event.target.checked })}
              />
            </td>
            <td>
              <input
                aria-label={`Name ${index + 1}`}
                spellCheck={false}
                {...unresolvedAttrs(row.name, variables)}
                value={row.name}
                onChange={(event) => update(index, { name: event.target.value })}
              />
            </td>
            <td>
              <input
                aria-label={`Value ${index + 1}`}
                spellCheck={false}
                {...unresolvedAttrs(row.value, variables)}
                value={row.value}
                onChange={(event) => update(index, { value: event.target.value })}
              />
            </td>
            <td>
              <Button label="Delete" aria-label={`Delete row ${index + 1}`} onClick={() => remove(index)} />
            </td>
          </tr>
        ))}
        {needsTrailing ? (
          <tr>
            <td />
            <td>
              <input
                aria-label={`Name ${rows.length + 1}`}
                spellCheck={false}
                value=""
                onChange={(event) => editTrailing("name", event.target.value)}
              />
            </td>
            <td>
              <input
                aria-label={`Value ${rows.length + 1}`}
                spellCheck={false}
                value=""
                onChange={(event) => editTrailing("value", event.target.value)}
              />
            </td>
            <td />
          </tr>
        ) : null}
      </tbody>
    </table>
  );
}
