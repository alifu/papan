import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { TabPanel } from "./TabPanel";
import { useTabsStore } from "./tabsStore";

describe("TabPanel", () => {
  beforeEach(() => {
    useTabsStore.setState(useTabsStore.getInitialState(), true);
  });

  it("shows the welcome page", () => {
    render(<TabPanel />);
    expect(screen.getByRole("heading", { name: "Welcome to Papan" })).toBeVisible();
    expect(screen.getAllByRole("listitem")).toHaveLength(3);
  });

  it("shows placeholder content for a blank tab", () => {
    useTabsStore.getState().newTab();
    render(<TabPanel />);
    expect(screen.getByText("Nothing to show yet.")).toBeVisible();
    expect(screen.getByRole("tabpanel")).toHaveAttribute("aria-labelledby", "tab-button-tab-1");
  });

  it("shows an empty state with a New Tab button when no tabs are open", async () => {
    useTabsStore.getState().closeTab("welcome");
    render(<TabPanel />);
    expect(screen.getByText("No open tabs.")).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: "New Tab" }));
    expect(useTabsStore.getState().tabs).toHaveLength(1);
  });
});
