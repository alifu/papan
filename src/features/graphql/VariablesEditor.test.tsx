import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { VariablesEditor, variablesWarning } from "./VariablesEditor";

vi.mock("../../ui/code/CodeEditor", () => import("../../ui/code/CodeEditor.mock"));

describe("variablesWarning", () => {
  it.each([
    ["", null],
    ["  ", null],
    ['{ "id": "{{userId}}" }', null],
    ["{}", null],
    ["[1]", "Variables should be a JSON object."],
    ["3", "Variables should be a JSON object."],
    ["null", "Variables should be a JSON object."],
    ["{ id: 1 }", "Variables are not valid JSON."],
  ])("%j gives %j", (text, expected) => {
    expect(variablesWarning(text)).toBe(expected);
  });
});

describe("VariablesEditor", () => {
  it("edits the text and warns about invalid JSON", async () => {
    const onChange = vi.fn();
    const { rerender } = render(<VariablesEditor value="" onChange={onChange} />);
    expect(screen.queryByRole("status")).toBeNull();
    await userEvent.type(screen.getByLabelText("GraphQL variables"), "x");
    expect(onChange).toHaveBeenCalledWith("x");
    rerender(<VariablesEditor value="{ nope" onChange={onChange} />);
    expect(screen.getByRole("status")).toHaveTextContent("not valid JSON");
  });
});
