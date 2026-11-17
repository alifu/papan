import { describe, expect, it } from "vitest";
import type { Variable } from "../../ipc/bindings";
import {
  emptyRow,
  isDirty,
  rowFromVariable,
  toInputs,
  validateRows,
  withTrailing,
  type VariableRow,
} from "./variableRows";

const variable = (over: Partial<Variable> = {}): Variable => ({
  name: "a",
  value: "1",
  current: null,
  enabled: true,
  secret: false,
  has_secret: false,
  ...over,
});
const named = (name: string): VariableRow => ({ ...emptyRow(), name });

describe("variableRows", () => {
  it("maps a plain variable to an input and back, keeping previous_name", () => {
    const row = { ...rowFromVariable(variable()), name: "b" };
    expect(toInputs([row])[0]).toEqual({
      name: "b",
      previous_name: "a",
      value: "1",
      current: null,
      enabled: true,
      secret: false,
      secret_edit: { op: "keep" },
    });
  });

  it("drops the empty trailing row and appends one when missing", () => {
    expect(toInputs([rowFromVariable(variable()), emptyRow()])).toHaveLength(1);
    const rows = withTrailing([rowFromVariable(variable())]);
    expect(rows).toHaveLength(2);
    expect(withTrailing(rows)).toBe(rows);
  });

  it("produces secret edits: keep, set, clear", () => {
    const secret = rowFromVariable(variable({ value: "", secret: true, has_secret: true }));
    expect(toInputs([secret])[0].secret_edit).toEqual({ op: "keep" });
    expect(toInputs([{ ...secret, secretInput: "hunter2" }])[0].secret_edit).toEqual({ op: "set", value: "hunter2" });
    expect(toInputs([{ ...secret, secretCleared: true }])[0].secret_edit).toEqual({ op: "clear" });
    expect(toInputs([{ ...secret, secretInput: "x" }])[0]).toMatchObject({ value: "", current: null });
  });

  it("validates names: empty, reserved, braces, duplicates, length", () => {
    const rows = [named("ok"), named("ok"), named("$uuid"), named("a{b"), named("n".repeat(129))];
    const errors = validateRows(rows);
    expect(errors[rows[0].id]).toBeUndefined();
    expect(errors[rows[1].id]).toBe("Duplicate name.");
    expect(errors[rows[2].id]).toMatch(/\$/);
    expect(errors[rows[3].id]).toMatch(/braces/);
    expect(errors[rows[4].id]).toMatch(/128/);
    expect(validateRows([{ ...emptyRow(), value: "x" }])).not.toEqual({});
  });

  it("rejects the 501st variable", () => {
    const rows = Array.from({ length: 501 }, (_, i) => named(`v${i}`));
    expect(Object.values(validateRows(rows))).toContain("At most 500 variables per scope.");
  });

  it("detects edits against the saved copy", () => {
    const saved = withTrailing([rowFromVariable(variable())]);
    expect(isDirty(saved, saved)).toBe(false);
    expect(isDirty(saved.map((r) => ({ ...r, value: "2" })), saved)).toBe(true);
  });
});
