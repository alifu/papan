import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { scratchLocation } from "../features/locations";
import { TabStrip } from "./TabStrip";
import { useTabsStore } from "./tabsStore";

vi.mock("../features/scratch/api", () => ({
  createScratch: vi
    .fn()
    .mockResolvedValue({ id: "s2", name: "Untitled 1", method: "GET" }),
}));

const openUntitled = () =>
  useTabsStore.getState().openRequest(scratchLocation("s1"), "Untitled 1");

describe("TabStrip", () => {
  beforeEach(() => {
    useTabsStore.setState(useTabsStore.getInitialState(), true);
  });

  it("renders tabs with aria-selected", () => {
    openUntitled();
    render(<TabStrip />);

    expect(screen.getByRole("tab", { name: "Welcome" })).toHaveAttribute(
      "aria-selected",
      "false",
    );
    expect(screen.getByRole("tab", { name: "Untitled 1" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("activates on click", async () => {
    openUntitled();
    render(<TabStrip />);
    await userEvent.click(screen.getByRole("tab", { name: "Welcome" }));
    expect(useTabsStore.getState().activeTabId).toBe("welcome");
  });

  it("activates neighbours with arrow keys, wrapping", async () => {
    openUntitled();
    render(<TabStrip />);
    screen.getByRole("tab", { name: "Untitled 1" }).focus();

    await userEvent.keyboard("{ArrowRight}");
    expect(useTabsStore.getState().activeTabId).toBe("welcome");
    expect(screen.getByRole("tab", { name: "Welcome" })).toHaveFocus();
    await userEvent.keyboard("{ArrowLeft}");
    expect(useTabsStore.getState().activeTabId).toBe("tab-1");
    await userEvent.keyboard("a");
    expect(useTabsStore.getState().activeTabId).toBe("tab-1");
  });

  it("closes a tab with its close button", async () => {
    render(<TabStrip />);
    await userEvent.click(
      screen.getByRole("button", { name: "Close Welcome" }),
    );
    expect(useTabsStore.getState().tabs).toHaveLength(0);
  });

  it("adds a tab with the new tab button", async () => {
    render(<TabStrip />);
    await userEvent.click(screen.getByRole("button", { name: "New tab" }));
    expect(
      await screen.findByRole("tab", { name: "Untitled 1" }),
    ).toBeVisible();
  });
});
