import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { KeyValueTable, type KeyValueRow } from "./KeyValueTable";

const ROWS: readonly KeyValueRow[] = [
  { id: "1", name: "a", value: "1", enabled: true },
  { id: "2", name: "b", value: "2", enabled: false },
];

const setup = (rows: readonly KeyValueRow[] = ROWS) => {
  const onChange = vi.fn();
  render(<KeyValueTable rows={rows} onChange={onChange} label="Params" />);
  return onChange;
};

describe("KeyValueTable", () => {
  it("renders rows plus an empty trailing row", () => {
    setup();
    expect(screen.getByLabelText("Name 1")).toHaveValue("a");
    expect(screen.getByLabelText("Name 3")).toHaveValue("");
  });
  it("does not add a second trailing row when the last is empty", () => {
    setup([...ROWS, { id: "3", name: "", value: "", enabled: true }]);
    expect(screen.queryByLabelText("Name 4")).toBeNull();
  });
  it("appends a row when typing in the trailing row", async () => {
    const onChange = setup();
    await userEvent.type(screen.getByLabelText("Value 3"), "z");
    expect(onChange).toHaveBeenCalledWith([...ROWS, expect.objectContaining({ name: "", value: "z", enabled: true })]);
  });
  it("edits, toggles and deletes without mutating the input", async () => {
    const onChange = setup();
    await userEvent.type(screen.getByLabelText("Name 1"), "x");
    expect(onChange).toHaveBeenLastCalledWith([{ ...ROWS[0], name: "ax" }, ROWS[1]]);
    await userEvent.click(screen.getByLabelText("Enable row 2"));
    expect(onChange).toHaveBeenLastCalledWith([ROWS[0], { ...ROWS[1], enabled: true }]);
    await userEvent.click(screen.getByRole("button", { name: "Delete row 1" }));
    expect(onChange).toHaveBeenLastCalledWith([ROWS[1]]);
    expect(ROWS[0]?.name).toBe("a");
  });
  it("flags cells that name undefined variables", () => {
    render(<KeyValueTable rows={[{ id: "1", name: "h", value: "{{nope}}", enabled: true }]} onChange={vi.fn()} label="H" variables={[]} />);
    expect(screen.getByLabelText("Value 1")).toHaveClass("has-unresolved");
    expect(screen.getByLabelText("Value 1")).toHaveAttribute("title", "Not defined: nope");
    expect(screen.getByLabelText("Name 1")).not.toHaveClass("has-unresolved");
  });
});
