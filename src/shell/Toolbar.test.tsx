import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installMatchMedia } from "../theme/matchMediaMock";
import { installMemoryStorage } from "../ui/memoryStorage";
import { useLayoutStore } from "./layoutStore";
import { useTabsStore } from "./tabsStore";
import { Toolbar } from "./Toolbar";

describe("Toolbar", () => {
  beforeEach(() => {
    installMemoryStorage();
    installMatchMedia(false);
    useLayoutStore.setState(useLayoutStore.getInitialState(), true);
    useTabsStore.setState(useTabsStore.getInitialState(), true);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("New Tab adds a tab", async () => {
    render(<Toolbar />);
    await userEvent.click(screen.getByRole("button", { name: "New Tab" }));
    expect(useTabsStore.getState().tabs).toHaveLength(2);
  });

  it("Toggle sidebar flips the collapsed state and aria-pressed", async () => {
    render(<Toolbar />);
    const toggle = screen.getByRole("button", { name: "Toggle sidebar" });
    expect(toggle).toHaveAttribute("aria-pressed", "true");

    await userEvent.click(toggle);

    expect(useLayoutStore.getState().sidebarCollapsed).toBe(true);
    expect(toggle).toHaveAttribute("aria-pressed", "false");
  });
});
