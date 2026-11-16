import { create } from "zustand";
import type { JourneyRunEvent, JourneySummary } from "../../ipc/bindings";
import { reportError } from "../../ui/toastStore";
import * as api from "./api";
import { applyRunEvent, EMPTY_RUN, type RunState } from "./runState";

export const journeyKey = (collectionId: string, file: string): string =>
  `${collectionId}/${file}`;

export interface JourneyState {
  readonly byCollection: Readonly<Record<string, readonly JourneySummary[]>>;
  readonly runs: Readonly<Record<string, RunState>>;
  load: (collectionId: string) => Promise<void>;
  remove: (collectionId: string, file: string) => Promise<boolean>;
  /** Starts a run; the steps arrive through `handleEvent`. */
  run: (collectionId: string, file: string) => Promise<void>;
  cancel: (collectionId: string, file: string) => Promise<void>;
  handleEvent: (event: JourneyRunEvent) => void;
  subscribe: () => Promise<() => void>;
}

export const useJourneyStore = create<JourneyState>()((set, get) => {
  const attempt = async <T>(call: () => Promise<T>): Promise<T | null> => {
    try {
      return await call();
    } catch (err) {
      reportError(err);
      return null;
    }
  };
  const setRun = (key: string, next: RunState) =>
    set({ runs: { ...get().runs, [key]: next } });
  return {
    byCollection: {},
    runs: {},
    load: async (collectionId) => {
      const list = await attempt(() => api.listJourneys(collectionId));
      if (list)
        set({ byCollection: { ...get().byCollection, [collectionId]: list } });
    },
    remove: async (collectionId, file) => {
      const done = await attempt(async () => {
        await api.deleteJourney(collectionId, file);
        return true;
      });
      await get().load(collectionId);
      return done === true;
    },
    run: async (collectionId, file) => {
      const key = journeyKey(collectionId, file);
      setRun(key, { ...EMPTY_RUN, running: true });
      const summary = await attempt(() => api.runJourney(collectionId, file));
      const now = get().runs[key] ?? EMPTY_RUN;
      // The events normally finish the run; this covers a run that failed to start.
      if (now.running)
        setRun(key, {
          ...now,
          running: false,
          finished: summary?.status ?? "failed",
          message: summary?.message ?? now.message,
        });
    },
    cancel: async (collectionId, file) => {
      await attempt(() => api.cancelJourney(collectionId, file));
    },
    handleEvent: ({ collection_id, file, event }) => {
      const key = journeyKey(collection_id, file);
      setRun(key, applyRunEvent(get().runs[key] ?? EMPTY_RUN, event));
    },
    subscribe: () => api.subscribeRun((event) => get().handleEvent(event)),
  };
});
