import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { MessageEditor } from "./MessageEditor";

vi.mock("../../ui/code/CodeEditor", () => import("../../ui/code/CodeEditor.mock"));

const method = { name: "Say", kind: "unary" as const, input_type: "a.In", output_type: "a.Out", template: '{"name":""}' };

describe("MessageEditor", () => {
  it("resets to the template once the message differs", async () => {
    const onChange = vi.fn();
    render(<MessageEditor message="{}" method={method} variables={[]} onChange={onChange} />);
    await userEvent.click(screen.getByRole("button", { name: "Reset to template" }));
    expect(onChange).toHaveBeenCalledWith('{"name":""}');
  });

  it("disables Reset while the message equals the template or no method is picked", () => {
    const { rerender } = render(<MessageEditor message='{"name":""}' method={method} variables={[]} onChange={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Reset to template" })).toBeDisabled();
    rerender(<MessageEditor message="{}" method={null} variables={[]} onChange={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Reset to template" })).toBeDisabled();
  });

  it("warns about invalid JSON but not about variables", () => {
    const { rerender } = render(<MessageEditor message="{oops" method={method} variables={[]} onChange={vi.fn()} />);
    expect(screen.getByRole("status")).toHaveTextContent("not valid JSON");
    rerender(<MessageEditor message='{"a": {{n}}}' method={method} variables={[]} onChange={vi.fn()} />);
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("reports edits", async () => {
    const onChange = vi.fn();
    render(<MessageEditor message="" method={method} variables={[]} onChange={onChange} />);
    await userEvent.type(screen.getByLabelText("Message"), "1");
    expect(onChange).toHaveBeenCalledWith("1");
  });
});
