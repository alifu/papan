import { create } from "zustand";
import type { HistoryEntry, ScratchItem } from "../../ipc/bindings";
import { reportError } from "../../ui/toastStore";
import * as api from "./api";

interface HistoryState {
  readonly entries: readonly HistoryEntry[];
  readonly load: () => Promise<void>;
  readonly clear: () => Promise<void>;
  /** Creates a scratch request from the entry; `null` when it failed (already reported). */
  readonly restore: (id: string) => Promise<ScratchItem | null>;
}

export const useHistoryStore = create<HistoryState>()((set) => ({
  entries: [],
  load: async () => {
    try {
      set({ entries: await api.listHistory() });
    } catch (err) {
      reportError(err);
    }
  },
  clear: async () => {
    try {
      await api.clearHistory();
      set({ entries: [] });
    } catch (err) {
      reportError(err);
    }
  },
  restore: async (id) => {
    try {
      return await api.restoreHistoryEntry(id);
    } catch (err) {
      reportError(err);
      return null;
    }
  },
}));
