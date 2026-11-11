import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { TabStrip } from "./TabStrip";
import { useTabsStore } from "./tabsStore";

describe("TabStrip", () => {
  beforeEach(() => {
    useTabsStore.setState(useTabsStore.getInitialState(), true);
  });

  it("renders tabs with aria-selected", () => {
    useTabsStore.getState().newTab();
    render(<TabStrip />);

    expect(screen.getByRole("tab", { name: "Welcome" })).toHaveAttribute("aria-selected", "false");
    expect(screen.getByRole("tab", { name: "Untitled 1" })).toHaveAttribute("aria-selected", "true");
  });

  it("activates on click", async () => {
    useTabsStore.getState().newTab();
    render(<TabStrip />);
    await userEvent.click(screen.getByRole("tab", { name: "Welcome" }));
    expect(useTabsStore.getState().activeTabId).toBe("welcome");
  });

  it("activates neighbours with arrow keys, wrapping", async () => {
    useTabsStore.getState().newTab();
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
    await userEvent.click(screen.getByRole("button", { name: "Close Welcome" }));
    expect(useTabsStore.getState().tabs).toHaveLength(0);
  });

  it("adds a tab with the new tab button", async () => {
    render(<TabStrip />);
    await userEvent.click(screen.getByRole("button", { name: "New tab" }));
    expect(screen.getByRole("tab", { name: "Untitled 1" })).toBeVisible();
  });
});
