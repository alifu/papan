import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { SaveToVariableDialog } from "./SaveToVariableDialog";

describe("SaveToVariableDialog", () => {
  it("inserts the generated line and closes", async () => {
    const onInsert = vi.fn();
    const onClose = vi.fn();
    render(<SaveToVariableDialog path={["data", "id"]} onInsert={onInsert} onClose={onClose} />);
    await userEvent.type(screen.getByLabelText("Variable name"), "token");
    await userEvent.selectOptions(screen.getByLabelText("Scope"), "global");
    await userEvent.click(screen.getByRole("button", { name: "Insert" }));
    expect(onInsert).toHaveBeenCalledWith('pm.globals.set("token", pm.response.json().data.id);');
    expect(onClose).toHaveBeenCalled();
  });

  it("does not insert without a name", () => {
    render(<SaveToVariableDialog path={["a"]} onInsert={vi.fn()} onClose={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Insert" })).toBeDisabled();
  });
});
