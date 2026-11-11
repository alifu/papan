import { beforeEach, describe, expect, it } from "vitest";
import { WELCOME_TAB, useTabsStore } from "./tabsStore";

const state = () => useTabsStore.getState();

describe("tabsStore", () => {
  beforeEach(() => {
    useTabsStore.setState(useTabsStore.getInitialState(), true);
  });

  it("starts with the welcome tab active", () => {
    expect(state().tabs).toEqual([WELCOME_TAB]);
    expect(state().activeTabId).toBe("welcome");
    expect(state().nextTabNumber).toBe(1);
  });

  it("newTab numbers, appends and activates", () => {
    const first = state().newTab();
    const second = state().newTab();

    expect(first).toBe("tab-1");
    expect(second).toBe("tab-2");
    expect(state().tabs.map((t) => t.title)).toEqual(["Welcome", "Untitled 1", "Untitled 2"]);
    expect(state().activeTabId).toBe("tab-2");
  });

  it("closing the active tab activates its right neighbour", () => {
    state().newTab();
    state().newTab();
    state().activateTab("tab-1");
    state().closeTab("tab-1");
    expect(state().activeTabId).toBe("tab-2");
  });

  it("closing the last active tab activates the new last tab", () => {
    state().newTab();
    state().closeTab("tab-1");
    expect(state().activeTabId).toBe("welcome");
  });

  it("closing the only tab leaves none active", () => {
    state().closeTab("welcome");
    expect(state().tabs).toEqual([]);
    expect(state().activeTabId).toBeNull();
  });

  it("closing an inactive tab keeps the active tab", () => {
    state().newTab();
    state().closeTab("welcome");
    expect(state().activeTabId).toBe("tab-1");
  });

  it("ignores unknown ids", () => {
    const before = state();
    state().closeTab("nope");
    state().activateTab("nope");
    expect(state().tabs).toBe(before.tabs);
    expect(state().activeTabId).toBe("welcome");
  });

  it("does not mutate the previous tabs array", () => {
    const before = state().tabs;
    state().newTab();
    expect(before).toEqual([WELCOME_TAB]);
  });
});
