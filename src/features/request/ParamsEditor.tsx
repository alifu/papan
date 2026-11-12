import { KeyValueTable, type KeyValueRow } from "./KeyValueTable";
import type { ParamRow } from "./params";

export interface ParamsEditorProps {
  readonly rows: readonly ParamRow[];
  readonly onChange: (rows: readonly ParamRow[]) => void;
}

/** Params table: a two-way view of the URL query string. */
export function ParamsEditor({ rows, onChange }: ParamsEditorProps) {
  const shown: readonly KeyValueRow[] = rows.map((row, index) => ({
    id: `param-${index}`,
    name: row.name,
    value: row.value,
    enabled: row.enabled,
  }));
  const handleChange = (next: readonly KeyValueRow[]) =>
    onChange(
      next.map((row, index) => {
        const old = rows[index];
        const same = old !== undefined && old.name === row.name && old.value === row.value;
        return {
          name: row.name,
          value: row.value,
          enabled: row.enabled,
          ...(same ? { hasEquals: old.hasEquals } : {}),
        };
      }),
    );
  return <KeyValueTable label="Query parameters" rows={shown} onChange={handleChange} />;
}
