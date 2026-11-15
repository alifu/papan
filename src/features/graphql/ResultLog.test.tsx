import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ResultLog } from "./ResultLog";
import type { SubResult } from "./subscriptionModel";

const row = (seq: number, over: Partial<SubResult> = {}): SubResult => ({
  seq,
  kind: "next",
  time: "10:00:00.000",
  size: 20,
  text: '{\n  "data": 1\n}',
  preview: '{ "data": 1 }',
  ...over,
});

describe("ResultLog", () => {
  it("shows an empty state and a count", () => {
    const { rerender } = render(<ResultLog results={[]} onClear={vi.fn()} />);
    expect(screen.getByText("No results yet.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Clear" })).toBeDisabled();
    rerender(<ResultLog results={[row(1), row(2), row(3)]} onClear={vi.fn()} />);
    expect(screen.getByRole("status")).toHaveTextContent("3 results");
  });

  it("expands a row to pretty JSON", async () => {
    render(<ResultLog results={[row(2)]} onClear={vi.fn()} />);
    expect(screen.getByText("10:00:00.000")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Expand result 2" }));
    expect(screen.getByLabelText("Result 2 body")).toHaveTextContent('"data": 1');
    await userEvent.click(screen.getByRole("button", { name: "Collapse result 2" }));
    expect(screen.queryByLabelText("Result 2 body")).toBeNull();
  });

  it("filters by text", async () => {
    render(<ResultLog results={[row(1), row(2, { text: "boom", preview: "boom", kind: "error" })]} onClear={vi.fn()} />);
    await userEvent.type(screen.getByLabelText("Filter text"), "boom");
    expect(screen.getAllByRole("listitem")).toHaveLength(1);
    await userEvent.clear(screen.getByLabelText("Filter text"));
    await userEvent.type(screen.getByLabelText("Filter text"), "zzz");
    expect(screen.getByText("No results match the filter.")).toBeInTheDocument();
  });

  it("clears", async () => {
    const onClear = vi.fn();
    render(<ResultLog results={[row(1)]} onClear={onClear} />);
    await userEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(onClear).toHaveBeenCalled();
  });
});
