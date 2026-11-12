import { beforeEach, describe, expect, it, vi } from "vitest";
import { collectionLocation, scratchLocation } from "../features/locations";
import { useScratchStore } from "../features/scratch/scratchStore";
import { WELCOME_TAB, useTabsStore } from "./tabsStore";

vi.mock("../features/scratch/api", () => ({
  createScratch: vi.fn().mockResolvedValue({ id: "s1", name: "Untitled", method: "GET" }),
}));

const state = () => useTabsStore.getState();
const loc = (path: string) => collectionLocation("c1", path);

describe("tabsStore", () => {
  beforeEach(() => {
    useTabsStore.setState(useTabsStore.getInitialState(), true);
    useScratchStore.setState({ items: [] });
  });

  it("starts with the welcome tab active", () => {
    expect(state().tabs).toEqual([WELCOME_TAB]);
    expect(state().activeTabId).toBe("welcome");
  });

  it("openRequest appends and activates a request tab", () => {
    const before = state().tabs;
    const id = state().openRequest(loc("a.http.yaml"), "A");

    expect(id).toBe("tab-1");
    expect(before).toEqual([WELCOME_TAB]);
    expect(state().tabs.map((t) => t.title)).toEqual(["Welcome", "A"]);
    expect(state().activeTabId).toBe("tab-1");
  });

  it("openRequest focuses the existing tab for the same location", () => {
    state().openRequest(loc("a.http.yaml"), "A");
    state().openRequest(loc("b.http.yaml"), "B");
    const id = state().openRequest(loc("a.http.yaml"), "A");

    expect(id).toBe("tab-1");
    expect(state().tabs).toHaveLength(3);
    expect(state().activeTabId).toBe("tab-1");
  });

  it("newTab creates a scratch request and opens it", async () => {
    await state().newTab();

    expect(state().tabs[1]).toMatchObject({ kind: "request", title: "Untitled", location: scratchLocation("s1") });
    expect(state().activeTabId).toBe("tab-1");
  });

  it("retarget moves a scratch tab to a collection location and renames it", () => {
    state().openRequest(scratchLocation("s1"), "Old");
    state().retarget(scratchLocation("s1"), loc("x.http.yaml"), "New");

    expect(state().tabs[1]).toMatchObject({ title: "New", location: loc("x.http.yaml") });
  });

  it("retarget rewrites tabs below a folder and keeps their titles", () => {
    state().openRequest(loc("auth/login.http.yaml"), "Login");
    state().openRequest(loc("other.http.yaml"), "Other");
    state().retarget(loc("auth"), loc("sec"), "ignored");

    expect(state().tabs[1]).toMatchObject({ title: "Login", location: loc("sec/login.http.yaml") });
    expect(state().tabs[2]).toMatchObject({ location: loc("other.http.yaml") });
  });

  it("retarget keeps the title when none is given", () => {
    state().openRequest(loc("a.http.yaml"), "A");
    state().retarget(loc("a.http.yaml"), loc("b.http.yaml"));
    expect(state().tabs[1]).toMatchObject({ title: "A", location: loc("b.http.yaml") });
  });

  it("closeByLocation closes the tab and tabs below a folder, and fixes the active tab", () => {
    state().openRequest(loc("auth/a.http.yaml"), "A");
    state().openRequest(loc("auth/b.http.yaml"), "B");
    state().closeByLocation(loc("auth"));

    expect(state().tabs).toEqual([WELCOME_TAB]);
    expect(state().activeTabId).toBe("welcome");
  });

  it("closeByLocation with an empty path closes every tab of the collection", () => {
    state().openRequest(loc("a.http.yaml"), "A");
    state().openRequest(collectionLocation("c2", "a.http.yaml"), "Other");
    state().closeByLocation(loc(""));
    expect(state().tabs.map((t) => t.title)).toEqual(["Welcome", "Other"]);
  });

  it("closeByLocation ignores unrelated locations", () => {
    const before = state().tabs;
    state().closeByLocation(loc("zzz"));
    expect(state().tabs).toBe(before);
  });

  it("closing the active tab activates its right neighbour", () => {
    state().openRequest(loc("a.http.yaml"), "A");
    state().openRequest(loc("b.http.yaml"), "B");
    state().activateTab("tab-1");
    state().closeTab("tab-1");
    expect(state().activeTabId).toBe("tab-2");
  });

  it("closing the last active tab activates the new last tab", () => {
    state().openRequest(loc("a.http.yaml"), "A");
    state().closeTab("tab-1");
    expect(state().activeTabId).toBe("welcome");
  });

  it("closing the only tab leaves none active", () => {
    state().closeTab("welcome");
    expect(state().tabs).toEqual([]);
    expect(state().activeTabId).toBeNull();
  });

  it("closing an inactive tab keeps the active tab", () => {
    state().openRequest(loc("a.http.yaml"), "A");
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
});
