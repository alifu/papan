import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { WsTemplate } from "../../ipc/bindings";
import { TemplatesEditor } from "./TemplatesEditor";

const templates: WsTemplate[] = [
  { name: "Join", format: "json", text: '{"t":"join"}' },
  { name: "Ping", format: "text", text: "ping" },
];

describe("TemplatesEditor", () => {
  it("shows an empty hint", () => {
    render(<TemplatesEditor templates={[]} onChange={vi.fn()} onLoad={vi.fn()} />);
    expect(screen.getByText(/No saved messages/)).toBeVisible();
  });

  it("loads a template into the composer", async () => {
    const onLoad = vi.fn();
    render(<TemplatesEditor templates={templates} onChange={vi.fn()} onLoad={onLoad} />);
    await userEvent.click(screen.getByRole("button", { name: "Load Ping into composer" }));
    expect(onLoad).toHaveBeenCalledWith(templates[1]);
  });

  it("renames without mutating the input", async () => {
    const onChange = vi.fn();
    render(<TemplatesEditor templates={templates} onChange={onChange} onLoad={vi.fn()} />);
    await userEvent.type(screen.getByLabelText("Name of Join"), "!");
    expect(onChange).toHaveBeenLastCalledWith([{ ...templates[0], name: "Join!" }, templates[1]]);
    expect(templates[0]?.name).toBe("Join");
  });

  it("flags an empty or duplicate name", () => {
    render(
      <TemplatesEditor
        templates={[templates[0]!, { ...templates[1]!, name: "Join" }]}
        onChange={vi.fn()}
        onLoad={vi.fn()}
      />,
    );
    expect(screen.getAllByRole("alert")[0]).toHaveTextContent(/already exists/);
  });

  it("reorders and deletes", async () => {
    const onChange = vi.fn();
    render(<TemplatesEditor templates={templates} onChange={onChange} onLoad={vi.fn()} />);
    await userEvent.click(screen.getByRole("button", { name: "Move Ping up" }));
    expect(onChange).toHaveBeenLastCalledWith([templates[1], templates[0]]);
    await userEvent.click(screen.getByRole("button", { name: "Delete Join" }));
    expect(onChange).toHaveBeenLastCalledWith([templates[1]]);
    expect(screen.getByRole("button", { name: "Move Join up" })).toBeDisabled();
  });
});
