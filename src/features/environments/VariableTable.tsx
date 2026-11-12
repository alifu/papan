import { Button } from "../../ui/Button";
import type { VariableRow } from "./variableRows";
import { withTrailing } from "./variableRows";

export interface VariableTableProps {
  readonly rows: readonly VariableRow[];
  readonly errors: Readonly<Record<string, string>>;
  readonly showCurrent: boolean;
  readonly onChange: (rows: readonly VariableRow[]) => void;
}

export const GIT_WARNING = "This value may already be in Git history. Rotate it.";

function secretPlaceholder(row: VariableRow): string {
  if (row.secretCleared) return "Will be cleared";
  return row.hasSecret ? "Stored" : "Not set on this machine";
}

/** Editable variable rows for one scope; secret values are write-only. `rows` should end with an empty row (`withTrailing`). */
export function VariableTable({ rows, errors, showCurrent, onChange }: VariableTableProps) {
  const shown = rows;
  const update = (id: string, patch: Partial<VariableRow>) =>
    onChange(withTrailing(shown.map((r) => (r.id === id ? { ...r, ...patch } : r))));
  const remove = (id: string) => onChange(withTrailing(shown.filter((r) => r.id !== id)));

  return (
    <table className="kv-table var-table" aria-label="Variables">
      <thead>
        <tr>
          <th scope="col"><span className="visually-hidden">Enabled</span></th>
          <th scope="col">Name</th>
          <th scope="col">{showCurrent ? "Initial value" : "Value"}</th>
          {showCurrent ? <th scope="col">Current value</th> : null}
          <th scope="col">Secret</th>
          <th scope="col"><span className="visually-hidden">Actions</span></th>
        </tr>
      </thead>
      <tbody>
        {shown.map((row, index) => {
          const n = index + 1;
          const error = errors[row.id];
          return (
            <tr key={row.id}>
              <td>
                <input
                  type="checkbox"
                  aria-label={`Enable variable ${n}`}
                  checked={row.enabled}
                  onChange={(e) => update(row.id, { enabled: e.target.checked })}
                />
              </td>
              <td>
                <input
                  aria-label={`Variable name ${n}`}
                  aria-invalid={error !== undefined}
                  spellCheck={false}
                  value={row.name}
                  onChange={(e) => update(row.id, { name: e.target.value })}
                />
                {error ? <span className="field-error" role="alert">{error}</span> : null}
              </td>
              {row.secret ? (
                <td colSpan={showCurrent ? 2 : 1}>
                  <input
                    type="password"
                    aria-label={`Secret value ${n}`}
                    placeholder={secretPlaceholder(row)}
                    autoComplete="off"
                    value={row.secretInput}
                    onChange={(e) => update(row.id, { secretInput: e.target.value, secretCleared: false })}
                  />
                  <Button
                    label="Clear"
                    aria-label={`Clear secret ${n}`}
                    disabled={!row.hasSecret || row.secretCleared}
                    onClick={() => update(row.id, { secretInput: "", secretCleared: true })}
                  />
                </td>
              ) : (
                <>
                  <td>
                    <input
                      aria-label={`Variable value ${n}`}
                      spellCheck={false}
                      value={row.value}
                      onChange={(e) => update(row.id, { value: e.target.value })}
                    />
                  </td>
                  {showCurrent ? (
                    <td>
                      <input
                        aria-label={`Current value ${n}`}
                        spellCheck={false}
                        value={row.current ?? ""}
                        placeholder="Same as initial"
                        onChange={(e) => update(row.id, { current: e.target.value === "" ? null : e.target.value })}
                      />
                      <Button
                        label="Reset"
                        aria-label={`Reset current value ${n}`}
                        disabled={row.current === null}
                        onClick={() => update(row.id, { current: null })}
                      />
                    </td>
                  ) : null}
                </>
              )}
              <td>
                <input
                  type="checkbox"
                  aria-label={`Secret variable ${n}`}
                  checked={row.secret}
                  onChange={(e) => update(row.id, { secret: e.target.checked, secretInput: "", secretCleared: false })}
                />
                {row.secret && row.hadPlainValue ? <span className="field-error">{GIT_WARNING}</span> : null}
              </td>
              <td>
                <Button label="Delete" aria-label={`Delete variable ${n}`} onClick={() => remove(row.id)} />
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
