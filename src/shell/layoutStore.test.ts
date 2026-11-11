import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installMemoryStorage } from "../ui/memoryStorage";
import {
  LAYOUT_STORAGE_KEY,
  SIDEBAR_DEFAULT_WIDTH,
  SIDEBAR_MAX_WIDTH,
  SIDEBAR_MIN_WIDTH,
  clampSidebarWidth,
  loadLayout,
  useLayoutStore,
} from "./layoutStore";

describe("layoutStore", () => {
  let storage: Storage;

  beforeEach(() => {
    storage = installMemoryStorage();
    useLayoutStore.setState(useLayoutStore.getInitialState(), true);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  const persisted = () => JSON.parse(storage.getItem(LAYOUT_STORAGE_KEY) ?? "null");

  it("has default values", () => {
    const state = useLayoutStore.getState();
    expect(state.sidebarWidth).toBe(SIDEBAR_DEFAULT_WIDTH);
    expect(state.sidebarCollapsed).toBe(false);
    expect(state.activeSection).toBe("collections");
  });

  it.each([
    [50, SIDEBAR_MIN_WIDTH],
    [9999, SIDEBAR_MAX_WIDTH],
    [Number.NaN, SIDEBAR_DEFAULT_WIDTH],
    [300.6, 301],
  ])("clampSidebarWidth(%s) is %s", (input, expected) => {
    expect(clampSidebarWidth(input)).toBe(expected);
  });

  it("setSidebarWidth clamps and persists", () => {
    useLayoutStore.getState().setSidebarWidth(10);
    expect(useLayoutStore.getState().sidebarWidth).toBe(SIDEBAR_MIN_WIDTH);
    expect(persisted().sidebarWidth).toBe(SIDEBAR_MIN_WIDTH);
  });

  it("toggleSidebar flips and persists", () => {
    useLayoutStore.getState().toggleSidebar();
    expect(useLayoutStore.getState().sidebarCollapsed).toBe(true);
    expect(persisted().sidebarCollapsed).toBe(true);
    useLayoutStore.getState().toggleSidebar();
    expect(useLayoutStore.getState().sidebarCollapsed).toBe(false);
  });

  it("setActiveSection changes and persists", () => {
    useLayoutStore.getState().setActiveSection("mocks");
    expect(useLayoutStore.getState().activeSection).toBe("mocks");
    expect(persisted().activeSection).toBe("mocks");
  });

  it("loadLayout reads a valid snapshot and clamps width", () => {
    storage.setItem(
      LAYOUT_STORAGE_KEY,
      JSON.stringify({ sidebarWidth: 9000, sidebarCollapsed: true, activeSection: "journeys" }),
    );
    expect(loadLayout()).toEqual({
      sidebarWidth: SIDEBAR_MAX_WIDTH,
      sidebarCollapsed: true,
      activeSection: "journeys",
    });
  });

  it.each([["{not json"], [JSON.stringify({ sidebarWidth: "wide" })]])(
    "loadLayout falls back to defaults for %s",
    (raw) => {
      storage.setItem(LAYOUT_STORAGE_KEY, raw);
      expect(loadLayout().sidebarWidth).toBe(SIDEBAR_DEFAULT_WIDTH);
    },
  );

  it("loadLayout returns defaults when nothing is stored", () => {
    expect(loadLayout().activeSection).toBe("collections");
  });

  it("does not mutate the previous state object", () => {
    const before = useLayoutStore.getState();
    before.setSidebarWidth(400);
    expect(before.sidebarWidth).toBe(SIDEBAR_DEFAULT_WIDTH);
    expect(useLayoutStore.getState()).not.toBe(before);
  });
});
