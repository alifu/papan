import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { JsonTree } from "./JsonTree";

describe("JsonTree", () => {
  it("opens the first two levels and collapses deeper ones", () => {
    render(<JsonTree text='{"a":{"b":{"c":1}}}' />);
    expect(screen.getByText("b")).toBeInTheDocument();
    expect(screen.queryByText("c")).toBeNull();
  });
  it("expands and collapses a node", async () => {
    render(<JsonTree text='{"a":{"b":{"c":1}}}' />);
    const toggle = screen.getByRole("button", { name: "b {1}" });
    await userEvent.click(toggle);
    expect(screen.getByText("c")).toBeInTheDocument();
    await userEvent.click(toggle);
    expect(screen.queryByText("c")).toBeNull();
  });
  it("pages long arrays", async () => {
    const text = JSON.stringify({ list: Array.from({ length: 250 }, (_, i) => `v${i}`) });
    render(<JsonTree text={text} />);
    expect(screen.queryByText('"v249"')).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: /Show more \(50 left\)/ }));
    expect(screen.getByText('"v249"')).toBeInTheDocument();
  });
  it("reports invalid JSON", () => {
    render(<JsonTree text="{" />);
    expect(screen.getByText("Not valid JSON.")).toBeInTheDocument();
  });
});

describe("JsonTree save to variable", () => {
  it("reports the path of the right-clicked node", async () => {
    const onSave = vi.fn();
    render(<JsonTree text='{"items":[7]}' onSaveToVariable={onSave} />);
    fireEvent.contextMenu(screen.getByText("0"));
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave).toHaveBeenCalledWith(["items", 0]);
  });
});
