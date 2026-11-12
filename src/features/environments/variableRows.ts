import { z } from "zod";
import type { SecretEdit, Variable, VariableInput } from "../../ipc/bindings";
import { MAX_NAME_LENGTH } from "./template";

export const MAX_VARIABLES = 500;

/** One editable row. Secret values are write-only: `secretInput` is what the user typed this session. */
export interface VariableRow {
  readonly id: string;
  readonly name: string;
  /** The name the variable had when loaded; `null` for a new row. */
  readonly previousName: string | null;
  readonly value: string;
  readonly current: string | null;
  readonly enabled: boolean;
  readonly secret: boolean;
  /** A secret value is stored on this machine. */
  readonly hasSecret: boolean;
  readonly secretInput: string;
  readonly secretCleared: boolean;
  /** The row was a plain variable with a value when loaded (so making it secret keeps that value). */
  readonly hadPlainValue: boolean;
}

export const nameSchema = z
  .string()
  .min(1, "Name is required.")
  .refine((n) => [...n].length <= MAX_NAME_LENGTH, `Name must be ${MAX_NAME_LENGTH} characters or fewer.`)
  .refine((n) => !/[{}\r\n]/.test(n), "Name cannot contain braces or line breaks.")
  .refine((n) => !n.startsWith("$"), "Names starting with $ are reserved for dynamic variables.");

let counter = 0;
const nextId = (): string => `row-${(counter += 1)}`;

export function emptyRow(): VariableRow {
  return {
    id: nextId(),
    name: "",
    previousName: null,
    value: "",
    current: null,
    enabled: true,
    secret: false,
    hasSecret: false,
    secretInput: "",
    secretCleared: false,
    hadPlainValue: false,
  };
}

export function rowFromVariable(v: Variable): VariableRow {
  return {
    id: nextId(),
    name: v.name,
    previousName: v.name,
    value: v.value,
    current: v.current,
    enabled: v.enabled,
    secret: v.secret,
    hasSecret: v.has_secret,
    secretInput: "",
    secretCleared: false,
    hadPlainValue: !v.secret && v.value !== "",
  };
}

export const isEmptyRow = (row: VariableRow): boolean =>
  row.name === "" && row.value === "" && row.current === null && row.secretInput === "" && !row.secret;

function secretEdit(row: VariableRow): SecretEdit {
  if (!row.secret) return { op: "keep" };
  if (row.secretInput !== "") return { op: "set", value: row.secretInput };
  return row.secretCleared ? { op: "clear" } : { op: "keep" };
}

export function toInput(row: VariableRow): VariableInput {
  return {
    name: row.name,
    previous_name: row.previousName,
    value: row.secret ? "" : row.value,
    current: row.secret || row.current === "" ? null : row.current,
    enabled: row.enabled,
    secret: row.secret,
    secret_edit: secretEdit(row),
  };
}

/** Rows to send: the empty placeholder row is dropped. */
export const toInputs = (rows: readonly VariableRow[]): VariableInput[] =>
  rows.filter((r) => !isEmptyRow(r)).map(toInput);

/** Rows with a trailing empty row, so there is always somewhere to type. */
export function withTrailing(rows: readonly VariableRow[]): readonly VariableRow[] {
  const last = rows[rows.length - 1];
  return last && isEmptyRow(last) ? rows : [...rows, emptyRow()];
}

export const isDirty = (rows: readonly VariableRow[], saved: readonly VariableRow[]): boolean =>
  JSON.stringify(toInputs(rows)) !== JSON.stringify(toInputs(saved));

/** Error text per row id; rows that are fine are absent. */
export function validateRows(rows: readonly VariableRow[]): Readonly<Record<string, string>> {
  const errors: Record<string, string> = {};
  const used = rows.filter((r) => !isEmptyRow(r));
  const seen = new Set<string>();
  for (const row of used) {
    const parsed = nameSchema.safeParse(row.name);
    if (!parsed.success) errors[row.id] = parsed.error.issues[0]?.message ?? "Name is not valid.";
    else if (seen.has(row.name)) errors[row.id] = "Duplicate name.";
    seen.add(row.name);
  }
  if (used.length > MAX_VARIABLES) {
    const extra = used[MAX_VARIABLES];
    if (extra) errors[extra.id] = `At most ${MAX_VARIABLES} variables per scope.`;
  }
  return errors;
}
