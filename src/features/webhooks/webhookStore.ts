import { create } from "zustand";
import type {
  Capture,
  CaptureSummary,
  WebhookCaptured,
  WebhookConfig,
  WebhookListener,
  WebhookStopped,
} from "../../ipc/bindings";
import { useTabsStore } from "../../shell/tabsStore";
import { reportError, showToast } from "../../ui/toastStore";
import { scratchLocation } from "../locations";
import { useScratchStore } from "../scratch/scratchStore";
import * as api from "./api";
import { DEFAULT_CONFIG, MAX_CAPTURES_LIMIT, MAX_LISTENERS } from "./schema";

export interface WebhookState {
  readonly listeners: readonly WebhookListener[];
  /** Newest first. */
  readonly captures: Readonly<Record<string, readonly CaptureSummary[]>>;
  load: () => Promise<void>;
  create: () => Promise<WebhookListener | null>;
  save: (config: WebhookConfig) => Promise<boolean>;
  remove: (id: string) => Promise<boolean>;
  start: (id: string) => Promise<void>;
  stop: (id: string) => Promise<void>;
  loadCaptures: (id: string) => Promise<void>;
  clearCaptures: (id: string) => Promise<void>;
  readCapture: (id: string, captureId: string) => Promise<Capture | null>;
  replay: (id: string, captureId: string) => Promise<boolean>;
  handleCaptured: (event: WebhookCaptured) => void;
  handleStopped: (event: WebhookStopped) => void;
  subscribe: () => Promise<() => void>;
}

const idOf = (listener: WebhookListener): string => listener.config.id;

function upsert(list: readonly WebhookListener[], next: WebhookListener): readonly WebhookListener[] {
  const exists = list.some((l) => idOf(l) === idOf(next));
  return exists ? list.map((l) => (idOf(l) === idOf(next) ? next : l)) : [...list, next];
}

function patchListener(
  list: readonly WebhookListener[],
  id: string,
  patch: Partial<WebhookListener>,
): readonly WebhookListener[] {
  return list.map((l) => (idOf(l) === id ? { ...l, ...patch } : l));
}

export const useWebhookStore = create<WebhookState>()((set, get) => {
  const attempt = async <T,>(call: () => Promise<T>): Promise<T | null> => {
    try {
      return await call();
    } catch (err) {
      reportError(err);
      return null;
    }
  };
  const succeeded = async (call: () => Promise<unknown>): Promise<boolean> => {
    try {
      await call();
      return true;
    } catch (err) {
      reportError(err);
      return false;
    }
  };
  const replaceListener = (listener: WebhookListener): void =>
    set({ listeners: upsert(get().listeners, listener) });

  return {
    listeners: [],
    captures: {},
    load: async () => {
      const listeners = await attempt(api.listWebhooks);
      if (listeners) set({ listeners });
    },
    create: async () => {
      if (get().listeners.length >= MAX_LISTENERS) {
        showToast({ kind: "error", message: `At most ${MAX_LISTENERS} webhook listeners.` });
        return null;
      }
      const listener = await attempt(() => api.saveWebhook(DEFAULT_CONFIG));
      if (listener) replaceListener(listener);
      return listener;
    },
    save: async (config) => {
      const listener = await attempt(() => api.saveWebhook(config));
      if (!listener) {
        await get().load();
        return false;
      }
      replaceListener(listener);
      useTabsStore.getState().retitleWebhook(config.id, listener.config.name);
      return true;
    },
    remove: async (id) => {
      if (!(await succeeded(() => api.deleteWebhook(id)))) return false;
      set({
        listeners: get().listeners.filter((l) => idOf(l) !== id),
        captures: Object.fromEntries(Object.entries(get().captures).filter(([key]) => key !== id)),
      });
      useTabsStore.getState().closeWebhook(id);
      return true;
    },
    start: async (id) => {
      const listener = await attempt(() => api.startWebhook(id));
      if (listener) replaceListener(listener);
    },
    stop: async (id) => {
      if (await succeeded(() => api.stopWebhook(id))) {
        set({ listeners: patchListener(get().listeners, id, { running: false, url: null }) });
      }
    },
    loadCaptures: async (id) => {
      const list = await attempt(() => api.listCaptures(id));
      if (list) set({ captures: { ...get().captures, [id]: [...list].reverse() } });
    },
    clearCaptures: async (id) => {
      if (await succeeded(() => api.clearCaptures(id))) {
        set({
          captures: { ...get().captures, [id]: [] },
          listeners: patchListener(get().listeners, id, { captured: 0, unmatched: 0 }),
        });
      }
    },
    readCapture: (id, captureId) => attempt(() => api.readCapture(id, captureId)),
    replay: async (id, captureId) => {
      const result = await attempt(() => api.replayCapture(id, captureId));
      if (!result) return false;
      await useScratchStore.getState().load();
      useTabsStore.getState().openRequest(scratchLocation(result.item.id), result.item.name);
      if (result.warnings.length > 0) showToast({ kind: "info", message: result.warnings.join(" ") });
      return true;
    },
    handleCaptured: ({ listener_id, capture }) => {
      const current = get().captures[listener_id] ?? [];
      if (current.some((c) => c.id === capture.id)) return;
      const listener = get().listeners.find((l) => idOf(l) === listener_id);
      const limit = listener?.config.max_captures ?? MAX_CAPTURES_LIMIT;
      set({
        captures: { ...get().captures, [listener_id]: [capture, ...current].slice(0, limit) },
        listeners: listener
          ? patchListener(get().listeners, listener_id, { captured: listener.captured + 1 })
          : get().listeners,
      });
    },
    handleStopped: ({ listener_id, error }) => {
      set({ listeners: patchListener(get().listeners, listener_id, { running: false, url: null }) });
      if (error) reportError(error);
    },
    subscribe: async () => {
      const [offCaptured, offStopped] = await Promise.all([
        api.subscribeCaptured((e) => get().handleCaptured(e)),
        api.subscribeStopped((e) => get().handleStopped(e)),
      ]);
      return () => {
        offCaptured();
        offStopped();
      };
    },
  };
});
