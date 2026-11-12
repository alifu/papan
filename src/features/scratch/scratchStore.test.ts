import { beforeEach, describe, expect, it, vi } from "vitest";
import { useTabsStore } from "../../shell/tabsStore";
import { appError, makeTree } from "../../test/fixtures";
import { useToastStore } from "../../ui/toastStore";
import { useCollectionsStore } from "../collections/collectionsStore";
import { collectionLocation, scratchLocation } from "../locations";
import * as api from "./api";
import { useScratchStore } from "./scratchStore";

vi.mock("./api");

const state = () => useScratchStore.getState();
const item = (id: string, name: string) => ({ id, name, method: "GET" });

describe("scratchStore", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useScratchStore.setState({ items: [] });
    useTabsStore.setState(useTabsStore.getInitialState(), true);
    useCollectionsStore.setState(useCollectionsStore.getInitialState(), true);
    useToastStore.setState({ toasts: [] });
  });

  it("load stores the items; failure reports", async () => {
    vi.mocked(api.listScratch).mockResolvedValueOnce([item("a", "A")]);
    await state().load();
    expect(state().items).toEqual([item("a", "A")]);

    vi.mocked(api.listScratch).mockRejectedValueOnce(appError("x"));
    await state().load();
    expect(state().items).toHaveLength(1);
    expect(useToastStore.getState().toasts).toHaveLength(1);
  });

  it("create adds a sorted item and opens its tab", async () => {
    useScratchStore.setState({ items: [item("z", "Zed")] });
    vi.mocked(api.createScratch).mockResolvedValue(item("n", "Untitled"));
    await state().create();

    expect(state().items.map((i) => i.id)).toEqual(["n", "z"]);
    expect(useTabsStore.getState().tabs[1].location).toEqual(scratchLocation("n"));
  });

  it("create failure reports and opens nothing", async () => {
    vi.mocked(api.createScratch).mockRejectedValue(appError("x"));
    expect(await state().create()).toBeNull();
    expect(useTabsStore.getState().tabs).toHaveLength(1);
  });

  it("rename replaces the list and retitles the open tab", async () => {
    useTabsStore.getState().openRequest(scratchLocation("a"), "A");
    vi.mocked(api.renameScratch).mockResolvedValue([item("a", "B")]);
    expect(await state().rename("a", "B")).toBe(true);
    expect(state().items[0].name).toBe("B");
    expect(useTabsStore.getState().tabs[1].title).toBe("B");

    vi.mocked(api.renameScratch).mockRejectedValue(appError("x"));
    expect(await state().rename("a", "C")).toBe(false);
  });

  it("remove closes its tab", async () => {
    useTabsStore.getState().openRequest(scratchLocation("a"), "A");
    vi.mocked(api.deleteScratch).mockResolvedValue([]);
    expect(await state().remove("a")).toBe(true);
    expect(useTabsStore.getState().tabs).toHaveLength(1);

    vi.mocked(api.deleteScratch).mockRejectedValue(appError("x"));
    expect(await state().remove("a")).toBe(false);
  });

  it("moveToCollection updates both stores, retargets the tab and shows an info toast", async () => {
    useScratchStore.setState({ items: [item("a", "A")] });
    useTabsStore.getState().openRequest(scratchLocation("a"), "A");
    vi.mocked(api.moveScratchToCollection).mockResolvedValue({ tree: makeTree("c1"), path: "auth/a.http.yaml" });

    expect(await state().moveToCollection("a", "c1", "auth")).toBe(true);

    expect(state().items).toEqual([]);
    expect(useCollectionsStore.getState().trees[0].collection.id).toBe("c1");
    expect(useTabsStore.getState().tabs[1].location).toEqual(collectionLocation("c1", "auth/a.http.yaml"));
    expect(useToastStore.getState().toasts[0]).toMatchObject({ kind: "info" });
  });

  it("moveToCollection failure changes nothing and reports", async () => {
    useScratchStore.setState({ items: [item("a", "A")] });
    useTabsStore.getState().openRequest(scratchLocation("a"), "A");
    vi.mocked(api.moveScratchToCollection).mockRejectedValue(appError("nope"));

    expect(await state().moveToCollection("a", "c1", "")).toBe(false);

    expect(state().items).toHaveLength(1);
    expect(useTabsStore.getState().tabs[1].location).toEqual(scratchLocation("a"));
    expect(useToastStore.getState().toasts[0]).toMatchObject({ kind: "error", message: "nope" });
  });
});
