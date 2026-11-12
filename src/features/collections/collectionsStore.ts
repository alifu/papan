import { create } from "zustand";
import type { CollectionTree, TreeChange } from "../../ipc/bindings";
import { reportError } from "../../ui/toastStore";
import * as api from "./api";

export type LoadStatus = "idle" | "loading" | "ready";

export const expandKey = (collectionId: string, path: string): string => `${collectionId}:${path}`;

export interface CollectionsState {
  readonly trees: readonly CollectionTree[];
  readonly expanded: ReadonlySet<string>;
  readonly status: LoadStatus;
  load: () => Promise<void>;
  create: (name: string) => Promise<CollectionTree | null>;
  open: () => Promise<CollectionTree | null>;
  close: (collectionId: string) => Promise<boolean>;
  createFolder: (collectionId: string, parent: string, name: string) => Promise<TreeChange | null>;
  createRequest: (collectionId: string, parent: string, name: string) => Promise<TreeChange | null>;
  rename: (collectionId: string, path: string, name: string) => Promise<TreeChange | null>;
  remove: (collectionId: string, path: string) => Promise<TreeChange | null>;
  move: (collectionId: string, path: string, folder: string, index: number) => Promise<TreeChange | null>;
  refresh: (collectionId: string) => Promise<void>;
  replaceTree: (tree: CollectionTree) => void;
  toggle: (key: string) => void;
  expand: (key: string) => void;
  subscribe: () => Promise<() => void>;
}

function withTree(trees: readonly CollectionTree[], tree: CollectionTree): readonly CollectionTree[] {
  const id = tree.collection.id;
  return trees.some((t) => t.collection.id === id)
    ? trees.map((t) => (t.collection.id === id ? tree : t))
    : [...trees, tree];
}

export const useCollectionsStore = create<CollectionsState>()((set, get) => {
  const addExpanded = (keys: readonly string[]) =>
    set({ expanded: new Set([...get().expanded, ...keys]) });

  const replaceTree = (tree: CollectionTree) => set({ trees: withTree(get().trees, tree) });

  /** Runs a call, reports failure as a toast and returns null so state stays unchanged. */
  const attempt = async <T,>(call: () => Promise<T>): Promise<T | null> => {
    try {
      return await call();
    } catch (err) {
      reportError(err);
      return null;
    }
  };

  const applyChange = async (
    call: () => Promise<TreeChange>,
    expandKeys: readonly string[] = [],
  ): Promise<TreeChange | null> => {
    const change = await attempt(call);
    if (!change) return null;
    replaceTree(change.tree);
    if (expandKeys.length > 0) addExpanded(expandKeys);
    return change;
  };

  const adopt = async (call: () => Promise<CollectionTree | null>) => {
    const tree = await attempt(call);
    if (!tree) return null;
    replaceTree(tree);
    addExpanded([expandKey(tree.collection.id, "")]);
    return tree;
  };

  return {
    trees: [],
    expanded: new Set<string>(),
    status: "idle",
    load: async () => {
      set({ status: "loading" });
      const trees = await attempt(api.listCollections);
      set({ trees: trees ?? get().trees, status: "ready" });
    },
    create: (name) => adopt(() => api.createCollection(name)),
    open: () => adopt(() => api.openCollection()),
    close: async (collectionId) => {
      try {
        await api.closeCollection(collectionId);
      } catch (err) {
        reportError(err);
        return false;
      }
      set({ trees: get().trees.filter((t) => t.collection.id !== collectionId) });
      return true;
    },
    createFolder: (id, parent, name) =>
      applyChange(() => api.createFolder(id, parent, name), [expandKey(id, parent)]),
    createRequest: (id, parent, name) =>
      applyChange(() => api.createRequest(id, parent, name), [expandKey(id, parent)]),
    rename: (id, path, name) => applyChange(() => api.renameNode(id, path, name)),
    remove: (id, path) => applyChange(() => api.deleteNode(id, path)),
    move: (id, path, folder, index) =>
      applyChange(() => api.moveNode(id, path, folder, index), [expandKey(id, folder)]),
    refresh: async (collectionId) => {
      const tree = await attempt(() => api.readTree(collectionId));
      if (tree) replaceTree(tree);
    },
    replaceTree,
    toggle: (key) => {
      const next = new Set(get().expanded);
      if (!next.delete(key)) next.add(key);
      set({ expanded: next });
    },
    expand: (key) => addExpanded([key]),
    subscribe: () =>
      api.subscribeCollectionChanged((collectionId) => {
        void get().refresh(collectionId);
      }),
  };
});
