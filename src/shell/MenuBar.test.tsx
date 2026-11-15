import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installMemoryStorage } from "../ui/memoryStorage";
import { useDialogsStore } from "./dialogsStore";
import { useLayoutStore } from "./layoutStore";
import { MenuBar } from "./MenuBar";
import { useTabsStore } from "./tabsStore";

vi.mock("../features/scratch/api", () => ({
  createScratch: vi.fn().mockResolvedValue({ id: "s1", name: "Untitled", method: "GET" }),
}));

describe("MenuBar", () => {
  beforeEach(() => {
    installMemoryStorage();
    useLayoutStore.setState(useLayoutStore.getInitialState(), true);
    useTabsStore.setState(useTabsStore.getInitialState(), true);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("File > New Tab opens a tab", async () => {
    render(<MenuBar />);
    await userEvent.click(screen.getByRole("menuitem", { name: "File" }));
    await userEvent.click(screen.getByRole("menuitem", { name: /New Tab/ }));
    await vi.waitFor(() => expect(useTabsStore.getState().tabs).toHaveLength(2));
  });

  it("File > Close Tab closes the active tab and is a no-op with none", async () => {
    render(<MenuBar />);
    for (const expected of [0, 0]) {
      await userEvent.click(screen.getByRole("menuitem", { name: "File" }));
      await userEvent.click(screen.getByRole("menuitem", { name: /Close Tab/ }));
      expect(useTabsStore.getState().tabs).toHaveLength(expected);
    }
  });

  it("View > Toggle Sidebar collapses the sidebar", async () => {
    render(<MenuBar />);
    await userEvent.click(screen.getByRole("menuitem", { name: "View" }));
    await userEvent.click(screen.getByRole("menuitem", { name: /Toggle Sidebar/ }));
    expect(useLayoutStore.getState().sidebarCollapsed).toBe(true);
  });

  it.each([
    ["File", /Import cURL/, "import-curl"],
    ["File", /Import Postman/, "import-postman"],
    ["File", /Export to Postman/, "export-postman"],
    ["File", /Clone Repository/, "clone-git"],
    ["Edit", /Network settings/, "network"],
    ["Request", /Manage cookies/, "cookies"],
  ])("%s menu opens the %s dialog", async (menu, item, dialog) => {
    useDialogsStore.setState({ open: null });
    render(<MenuBar />);
    await userEvent.click(screen.getByRole("menuitem", { name: menu }));
    await userEvent.click(screen.getByRole("menuitem", { name: item }));
    expect(useDialogsStore.getState().open).toBe(dialog);
  });
});
