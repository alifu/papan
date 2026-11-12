import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TabPanel } from "./TabPanel";
import { useTabsStore } from "./tabsStore";

vi.mock("../features/scratch/api", () => ({
  createScratch: vi.fn().mockResolvedValue({ id: "s1", name: "Untitled", method: "GET" }),
}));
vi.mock("../features/collections/api", () => ({
  readRequest: vi.fn().mockResolvedValue({ name: "Untitled", method: "GET", url: "" }),
  writeRequest: vi.fn(),
  subscribeCollectionChanged: vi.fn(),
}));

describe("TabPanel", () => {
  beforeEach(() => {
    useTabsStore.setState(useTabsStore.getInitialState(), true);
  });

  it("shows the welcome page", () => {
    render(<TabPanel />);
    expect(screen.getByRole("heading", { name: "Welcome to Papan" })).toBeVisible();
    expect(screen.getAllByRole("listitem")).toHaveLength(3);
  });

  it("renders the request editor for a request tab", async () => {
    useTabsStore.getState().openRequest({ scope: "scratch", id: "s1" }, "Untitled");
    render(<TabPanel />);
    expect(await screen.findByLabelText("URL")).toBeVisible();
    expect(screen.getByRole("tabpanel")).toHaveAttribute("aria-labelledby", "tab-button-tab-1");
  });

  it("shows an empty state with a New Tab button when no tabs are open", async () => {
    useTabsStore.getState().closeTab("welcome");
    render(<TabPanel />);
    expect(screen.getByText("No open tabs.")).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: "New Tab" }));
    await vi.waitFor(() => expect(useTabsStore.getState().tabs).toHaveLength(1));
  });
});
