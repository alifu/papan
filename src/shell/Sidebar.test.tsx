import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installMemoryStorage } from "../ui/memoryStorage";
import { useLayoutStore } from "./layoutStore";
import { Sidebar } from "./Sidebar";

describe("Sidebar", () => {
  beforeEach(() => {
    installMemoryStorage();
    useLayoutStore.setState(useLayoutStore.getInitialState(), true);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("renders four sections with only the active one expanded", () => {
    render(<Sidebar />);

    const headers = screen.getAllByRole("button", { expanded: true });
    expect(screen.getByRole("button", { name: "Collections" })).toHaveAttribute("aria-expanded", "true");
    expect(headers).toHaveLength(1);
    for (const name of ["Scratch", "Mocks", "Journeys"]) {
      expect(screen.getByRole("button", { name })).toHaveAttribute("aria-expanded", "false");
    }
    expect(screen.getByRole("region", { name: "Collections" })).toHaveTextContent("No collections yet.");
    expect(screen.getAllByRole("region")).toHaveLength(1);
  });

  it("switches section when a header is clicked", async () => {
    render(<Sidebar />);
    await userEvent.click(screen.getByRole("button", { name: "Mocks" }));

    expect(screen.getByRole("region", { name: "Mocks" })).toHaveTextContent("No mock servers yet.");
    expect(screen.queryByText("No collections yet.")).toBeNull();
    expect(useLayoutStore.getState().activeSection).toBe("mocks");
  });

  it("collapses to a rail and expands again", async () => {
    render(<Sidebar />);
    await userEvent.click(screen.getByRole("button", { name: "Collapse sidebar" }));

    expect(screen.queryByRole("button", { name: "Collections" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Expand sidebar" }));
    expect(screen.getByRole("button", { name: "Collections" })).toBeVisible();
  });
});
