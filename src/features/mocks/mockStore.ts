import { create } from "zustand";
import type { MockLogEntry, MockLogged, MockSummary } from "../../ipc/bindings";
import { reportError } from "../../ui/toastStore";
import * as api from "./api";

/** Entries kept per mock in the UI; matches the Rust ring. */
export const MAX_LOG_IN_UI = 500;

export const mockKey = (collectionId: string, file: string): string =>
  `${collectionId}/${file}`;

export interface MockState {
  readonly byCollection: Readonly<Record<string, readonly MockSummary[]>>;
  /** Newest last. */
  readonly logs: Readonly<Record<string, readonly MockLogEntry[]>>;
  load: (collectionId: string) => Promise<void>;
  loadLog: (collectionId: string, file: string) => Promise<void>;
  clearLog: (collectionId: string, file: string) => Promise<void>;
  start: (collectionId: string, file: string) => Promise<boolean>;
  stop: (collectionId: string, file: string) => Promise<void>;
  remove: (collectionId: string, file: string) => Promise<boolean>;
  handleLogged: (event: MockLogged) => void;
  subscribe: () => Promise<() => void>;
}

export const useMockStore = create<MockState>()((set, get) => {
  const attempt = async <T>(call: () => Promise<T>): Promise<T | null> => {
    try {
      return await call();
    } catch (err) {
      reportError(err);
      return null;
    }
  };
  return {
    byCollection: {},
    logs: {},
    load: async (collectionId) => {
      const list = await attempt(() => api.listMocks(collectionId));
      if (list)
        set({ byCollection: { ...get().byCollection, [collectionId]: list } });
    },
    loadLog: async (collectionId, file) => {
      const entries = await attempt(() => api.mockLog(collectionId, file));
      if (entries)
        set({
          logs: { ...get().logs, [mockKey(collectionId, file)]: entries },
        });
    },
    clearLog: async (collectionId, file) => {
      if (
        (await attempt(() => api.clearMockLog(collectionId, file))) !== null
      ) {
        set({ logs: { ...get().logs, [mockKey(collectionId, file)]: [] } });
      }
    },
    start: async (collectionId, file) => {
      const url = await attempt(() => api.startMock(collectionId, file));
      await get().load(collectionId);
      return url !== null;
    },
    stop: async (collectionId, file) => {
      await attempt(() => api.stopMock(collectionId, file));
      await get().load(collectionId);
    },
    remove: async (collectionId, file) => {
      const done = await attempt(() => api.deleteMock(collectionId, file));
      await get().load(collectionId);
      return done !== null;
    },
    handleLogged: ({ collection_id, file, entry }) => {
      const key = mockKey(collection_id, file);
      const kept = [...(get().logs[key] ?? []), entry].slice(-MAX_LOG_IN_UI);
      set({ logs: { ...get().logs, [key]: kept } });
    },
    subscribe: () => api.subscribeLogged((event) => get().handleLogged(event)),
  };
});
