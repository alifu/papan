import { fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installMemoryStorage } from "../ui/memoryStorage";
import { useLayoutStore } from "./layoutStore";
import { useShortcuts } from "./useShortcuts";
import { useTabsStore } from "./tabsStore";

function Harness() {
  useShortcuts();
  return null;
}

function press(init: KeyboardEventInit): boolean {
  const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
  window.dispatchEvent(event);
  return event.defaultPrevented;
}

vi.mock("../features/scratch/api", () => ({
  createScratch: vi.fn().mockResolvedValue({ id: "s1", name: "Untitled", method: "GET" }),
}));

describe("useShortcuts", () => {
  beforeEach(() => {
    installMemoryStorage();
    useLayoutStore.setState(useLayoutStore.getInitialState(), true);
    useTabsStore.setState(useTabsStore.getInitialState(), true);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  describe.each([
    ["Mac", "MacIntel", { metaKey: true }, { ctrlKey: true }],
    ["non-Mac", "Win32", { ctrlKey: true }, { metaKey: true }],
  ])("on %s", (_name, platform, mod, wrongMod) => {
    beforeEach(() => {
      vi.stubGlobal("navigator", { platform });
      render(<Harness />);
    });

    it("Mod+N opens a tab", async () => {
      expect(press({ key: "n", ...mod })).toBe(true);
      await vi.waitFor(() => expect(useTabsStore.getState().tabs).toHaveLength(2));
    });

    it("Mod+W closes the active tab, and is harmless with none", () => {
      expect(press({ key: "w", ...mod })).toBe(true);
      expect(useTabsStore.getState().tabs).toHaveLength(0);
      expect(press({ key: "w", ...mod })).toBe(true);
    });

    it("Mod+B toggles the sidebar", () => {
      expect(press({ key: "b", ...mod })).toBe(true);
      expect(useLayoutStore.getState().sidebarCollapsed).toBe(true);
    });

    it("ignores Shift, Alt, the wrong modifier and other keys", () => {
      expect(press({ key: "n", shiftKey: true, ...mod })).toBe(false);
      expect(press({ key: "n", altKey: true, ...mod })).toBe(false);
      expect(press({ key: "n", ...wrongMod })).toBe(false);
      expect(press({ key: "n" })).toBe(false);
      expect(press({ key: "x", ...mod })).toBe(false);
      expect(useTabsStore.getState().tabs).toHaveLength(1);
    });
  });

  it("removes the listener on unmount", () => {
    vi.stubGlobal("navigator", { platform: "Win32" });
    const { unmount } = render(<Harness />);
    unmount();
    fireEvent.keyDown(window, { key: "n", ctrlKey: true });
    expect(useTabsStore.getState().tabs).toHaveLength(1);
  });
});
