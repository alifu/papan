import { beforeEach, describe, expect, it, vi } from "vitest";
import { appError, makeTree } from "../../test/fixtures";
import { useToastStore } from "../../ui/toastStore";
import * as api from "./api";
import { expandKey, useCollectionsStore } from "./collectionsStore";

vi.mock("./api");

const state = () => useCollectionsStore.getState();
const toasts = () => useToastStore.getState().toasts.map((t) => t.message);

describe("collectionsStore", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useCollectionsStore.setState(useCollectionsStore.getInitialState(), true);
    useToastStore.setState({ toasts: [] });
  });

  it("load stores the trees and becomes ready", async () => {
    vi.mocked(api.listCollections).mockResolvedValue([makeTree("c1"), makeTree("c2")]);
    await state().load();
    expect(state().trees.map((t) => t.collection.id)).toEqual(["c1", "c2"]);
    expect(state().status).toBe("ready");
  });

  it("load failure keeps state, reports and becomes ready", async () => {
    vi.mocked(api.listCollections).mockRejectedValue(appError("Disk gone"));
    await state().load();
    expect(state().trees).toEqual([]);
    expect(state().status).toBe("ready");
    expect(toasts()).toEqual(["Disk gone"]);
  });

  it("create appends the tree and expands it; null is a no-op", async () => {
    vi.mocked(api.createCollection).mockResolvedValueOnce(makeTree("c1"));
    await state().create("My API");
    expect(state().trees).toHaveLength(1);
    expect(state().expanded.has(expandKey("c1", ""))).toBe(true);

    vi.mocked(api.createCollection).mockResolvedValueOnce(null);
    const before = state();
    expect(await state().create("x")).toBeNull();
    expect(state().trees).toBe(before.trees);
    expect(toasts()).toEqual([]);
  });

  it("open replaces an already-listed tree instead of duplicating it", async () => {
    useCollectionsStore.setState({ trees: [makeTree("c1", "Old")] });
    vi.mocked(api.openCollection).mockResolvedValue(makeTree("c1", "New"));
    await state().open();
    expect(state().trees.map((t) => t.collection.name)).toEqual(["New"]);
  });

  it("close removes the tree and reports failures", async () => {
    useCollectionsStore.setState({ trees: [makeTree("c1"), makeTree("c2")] });
    vi.mocked(api.closeCollection).mockResolvedValueOnce(null);
    expect(await state().close("c1")).toBe(true);
    expect(state().trees.map((t) => t.collection.id)).toEqual(["c2"]);

    vi.mocked(api.closeCollection).mockRejectedValueOnce(appError("nope"));
    expect(await state().close("c2")).toBe(false);
    expect(state().trees).toHaveLength(1);
    expect(toasts()).toEqual(["nope"]);
  });

  it("a mutation replaces only that tree without mutating the old one", async () => {
    const c1 = makeTree("c1");
    const c2 = makeTree("c2");
    useCollectionsStore.setState({ trees: [c1, c2] });
    const updated = { ...c1, children: [] };
    vi.mocked(api.deleteNode).mockResolvedValue({ tree: updated, path: null });

    await state().remove("c1", "auth");

    expect(state().trees[0]).toBe(updated);
    expect(state().trees[1]).toBe(c2);
    expect(c1.children).toHaveLength(3);
  });

  it.each([
    ["createFolder", () => state().createFolder("c1", "auth", "n"), api.createFolder],
    ["createRequest", () => state().createRequest("c1", "auth", "n"), api.createRequest],
    ["rename", () => state().rename("c1", "auth", "n"), api.renameNode],
    ["move", () => state().move("c1", "health.http.yaml", "auth", 0), api.moveNode],
  ])("%s calls its command and reports failures without changing state", async (_n, run, fn) => {
    const tree = makeTree("c1");
    useCollectionsStore.setState({ trees: [tree] });
    vi.mocked(fn as typeof api.renameNode).mockRejectedValueOnce(appError("bad"));
    expect(await run()).toBeNull();
    expect(state().trees[0]).toBe(tree);
    expect(toasts()).toEqual(["bad"]);

    const next = { ...tree, children: [] };
    vi.mocked(fn as typeof api.renameNode).mockResolvedValueOnce({ tree: next, path: "p" });
    expect(await run()).toMatchObject({ path: "p" });
    expect(state().trees[0]).toBe(next);
  });

  it("creating inside a folder expands it", async () => {
    useCollectionsStore.setState({ trees: [makeTree("c1")] });
    vi.mocked(api.createRequest).mockResolvedValue({ tree: makeTree("c1"), path: "auth/n.http.yaml" });
    await state().createRequest("c1", "auth", "n");
    expect(state().expanded.has(expandKey("c1", "auth"))).toBe(true);
  });

  it("refresh reads only that collection", async () => {
    useCollectionsStore.setState({ trees: [makeTree("c1"), makeTree("c2")] });
    const fresh = { ...makeTree("c2"), children: [] };
    vi.mocked(api.readTree).mockResolvedValue(fresh);
    await state().refresh("c2");
    expect(api.readTree).toHaveBeenCalledWith("c2");
    expect(state().trees[1]).toBe(fresh);
  });

  it("toggle and expand update the set immutably", () => {
    const before = state().expanded;
    state().toggle("k");
    expect(state().expanded.has("k")).toBe(true);
    expect(before.has("k")).toBe(false);
    state().toggle("k");
    expect(state().expanded.has("k")).toBe(false);
    state().expand("k");
    expect(state().expanded.has("k")).toBe(true);
  });

  it("subscribe refreshes the collection named by the event", async () => {
    let emit: (id: string) => void = () => {};
    vi.mocked(api.subscribeCollectionChanged).mockImplementation(async (cb) => {
      emit = cb;
      return () => {};
    });
    vi.mocked(api.readTree).mockResolvedValue(makeTree("c9"));
    await state().subscribe();
    emit("c9");
    await vi.waitFor(() => expect(api.readTree).toHaveBeenCalledWith("c9"));
  });
});
