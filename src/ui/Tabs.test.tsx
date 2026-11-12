import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { Tabs } from "./Tabs";

const TABS = [
  { id: "a", label: "Params", count: 2 },
  { id: "b", label: "Headers" },
  { id: "c", label: "Body" },
];

function Harness() {
  const [id, setId] = useState("a");
  return (
    <Tabs tabs={TABS} selectedId={id} onSelect={setId} label="Request">
      <p>content {id}</p>
    </Tabs>
  );
}

describe("Tabs", () => {
  it("exposes tablist, tab and tabpanel roles with the selected tab", () => {
    render(<Harness />);
    expect(screen.getByRole("tablist", { name: "Request" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Params (2)" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tabpanel")).toHaveTextContent("content a");
  });

  it("selects a tab on click", async () => {
    render(<Harness />);
    await userEvent.click(screen.getByRole("tab", { name: "Body" }));
    expect(screen.getByRole("tabpanel")).toHaveTextContent("content c");
  });

  it("moves with arrow keys, wrapping, and Home/End", async () => {
    render(<Harness />);
    screen.getByRole("tab", { name: "Params (2)" }).focus();
    await userEvent.keyboard("{ArrowLeft}");
    expect(screen.getByRole("tab", { name: "Body" })).toHaveFocus();
    await userEvent.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Params (2)" })).toHaveFocus();
    await userEvent.keyboard("{End}");
    expect(screen.getByRole("tabpanel")).toHaveTextContent("content c");
    await userEvent.keyboard("{Home}");
    expect(screen.getByRole("tabpanel")).toHaveTextContent("content a");
  });

  it("keeps only the selected tab in the tab order", () => {
    render(<Harness />);
    expect(screen.getByRole("tab", { name: "Headers" })).toHaveAttribute("tabindex", "-1");
  });
});
