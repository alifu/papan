import { create } from "zustand";
import type { RequestKind, ScratchItem } from "../../ipc/bindings";
import { showToast, reportError } from "../../ui/toastStore";
import { useCollectionsStore } from "../collections/collectionsStore";
import { collectionLocation, scratchLocation } from "../locations";
import { useTabsStore } from "../../shell/tabsStore";
import * as api from "./api";

export interface ScratchState {
  readonly items: readonly ScratchItem[];
  load: () => Promise<void>;
  create: (kind?: RequestKind) => Promise<ScratchItem | null>;
  rename: (id: string, name: string) => Promise<boolean>;
  remove: (id: string) => Promise<boolean>;
  moveToCollection: (id: string, collectionId: string, folder: string) => Promise<boolean>;
}

const byNameThenId = (a: ScratchItem, b: ScratchItem): number =>
  a.name.toLowerCase().localeCompare(b.name.toLowerCase()) || a.id.localeCompare(b.id);

export const useScratchStore = create<ScratchState>()((set, get) => {
  const attempt = async <T,>(call: () => Promise<T>): Promise<T | null> => {
    try {
      return await call();
    } catch (err) {
      reportError(err);
      return null;
    }
  };

  return {
    items: [],
    load: async () => {
      const items = await attempt(api.listScratch);
      if (items) set({ items });
    },
    create: async (kind = "http") => {
      const item = await attempt(() => api.createScratch(kind));
      if (!item) return null;
      set({ items: [...get().items, item].sort(byNameThenId) });
      useTabsStore.getState().openRequest(scratchLocation(item.id), item.name);
      return item;
    },
    rename: async (id, name) => {
      const items = await attempt(() => api.renameScratch(id, name));
      if (!items) return false;
      set({ items });
      const loc = scratchLocation(id);
      const renamed = items.find((i) => i.id === id);
      useTabsStore.getState().retarget(loc, loc, renamed?.name ?? name);
      return true;
    },
    remove: async (id) => {
      const items = await attempt(() => api.deleteScratch(id));
      if (!items) return false;
      set({ items });
      useTabsStore.getState().closeByLocation(scratchLocation(id));
      return true;
    },
    moveToCollection: async (id, collectionId, folder) => {
      const change = await attempt(() => api.moveScratchToCollection(id, collectionId, folder));
      if (!change) return false;
      useCollectionsStore.getState().replaceTree(change.tree);
      set({ items: get().items.filter((i) => i.id !== id) });
      if (change.path !== null) {
        useTabsStore
          .getState()
          .retarget(scratchLocation(id), collectionLocation(collectionId, change.path));
      }
      showToast({ kind: "info", message: `Moved to ${change.tree.collection.name}.` });
      return true;
    },
  };
});
