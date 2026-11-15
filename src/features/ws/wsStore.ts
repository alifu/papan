import { create } from "zustand";
import type {
  AppError,
  RequestContent,
  RequestLocation,
  WsCloseBy,
  WsEvent,
  WsFormat,
  WsMessage,
  WsOpened,
  WsOutgoing,
} from "../../ipc/bindings";
import { isAppError } from "../../ipc/errors";
import { reportError } from "../../ui/toastStore";
import * as api from "./api";
import { mergeMessages } from "./wsModel";

export type WsStatus = "idle" | "connecting" | "open" | "closing" | "closed";

export interface WsClosedInfo {
  readonly code: number | null;
  readonly reason: string;
  readonly by: WsCloseBy;
}

export interface Composer {
  readonly format: WsFormat;
  readonly text: string;
}

export interface WsTab {
  readonly status: WsStatus;
  readonly opened: WsOpened | null;
  readonly log: readonly WsMessage[];
  readonly error: AppError | null;
  readonly closed: WsClosedInfo | null;
  readonly composer: Composer;
  /** The backend log was fetched for this tab (re-opened tab). */
  readonly restored: boolean;
}

export interface WsState {
  /** UI state keyed by tab id. */
  readonly byTab: Readonly<Record<string, WsTab>>;
  /** The live connection id of each open tab. */
  readonly connByTab: Readonly<Record<string, string>>;
  /** Stable UUID connection id per request location, so a re-opened request finds its log. */
  readonly idByLocation: Readonly<Record<string, string>>;
  connect: (tabId: string, location: RequestLocation, content: RequestContent) => Promise<void>;
  disconnect: (tabId: string) => Promise<void>;
  send: (tabId: string, location: RequestLocation, message: WsOutgoing) => Promise<void>;
  clear: (tabId: string) => Promise<void>;
  restore: (tabId: string, location: RequestLocation) => Promise<void>;
  setComposer: (tabId: string, patch: Partial<Composer>) => void;
  apply: (event: WsEvent) => void;
  /** Closes the tab's connection if live and forgets its state. */
  release: (tabId: string) => Promise<void>;
  drop: (tabId: string) => void;
  reset: () => void;
}

const NORMAL_CLOSE = 1000;
export const CLOSE_TIMEOUT_MS = 5000;
const locationKey = (location: RequestLocation): string => JSON.stringify(location);
const INTERNAL_ERROR: AppError = { code: "internal", message: "Something went wrong. Please try again.", detail: null };
const toAppError = (err: unknown): AppError => (isAppError(err) ? err : INTERNAL_ERROR);

const EMPTY_TAB: WsTab = {
  status: "idle",
  opened: null,
  log: [],
  error: null,
  closed: null,
  composer: { format: "text", text: "" },
  restored: false,
};

let listening: Promise<unknown> | null = null;
const closeTimers = new Map<string, ReturnType<typeof setTimeout>>();
const stopCloseTimer = (tabId: string) => {
  clearTimeout(closeTimers.get(tabId));
  closeTimers.delete(tabId);
};

export const useWsStore = create<WsState>()((set, get) => {
  const patch = (tabId: string, update: (tab: WsTab) => WsTab) =>
    set((s) => ({ byTab: { ...s.byTab, [tabId]: update(s.byTab[tabId] ?? EMPTY_TAB) } }));
  /** Like `patch`, but never recreates a tab that was released meanwhile. */
  const patchLive = (tabId: string, update: (tab: WsTab) => WsTab) => {
    if (get().byTab[tabId]) patch(tabId, update);
  };
  const bind = (tabId: string, location: RequestLocation): string => {
    const key = locationKey(location);
    const id = get().idByLocation[key] ?? crypto.randomUUID();
    set((s) => ({
      idByLocation: { ...s.idByLocation, [key]: id },
      connByTab: { ...s.connByTab, [tabId]: id },
    }));
    return id;
  };
  const ensureListening = () => {
    listening ??= api.listenWsEvents((event) => get().apply(event)).catch((err: unknown) => {
      listening = null;
      throw err;
    });
    return listening;
  };
  const forget = (tabId: string) => {
    stopCloseTimer(tabId);
    set((s) => {
      const byTab = { ...s.byTab };
      const connByTab = { ...s.connByTab };
      delete byTab[tabId];
      delete connByTab[tabId];
      return { byTab, connByTab };
    });
  };

  return {
    byTab: {},
    connByTab: {},
    idByLocation: {},
    connect: async (tabId, location, content) => {
      const connectionId = bind(tabId, location);
      stopCloseTimer(tabId);
      patch(tabId, (t) => ({ ...t, status: "connecting", opened: null, error: null, closed: null }));
      try {
        await ensureListening();
        const opened = await api.wsConnect(connectionId, location, content);
        patchLive(tabId, (t) => (t.status === "connecting" ? { ...t, status: "open", opened } : t));
      } catch (err) {
        if (!get().byTab[tabId]) return;
        const error = toAppError(err);
        const cancelled = error.code === "cancelled";
        patchLive(tabId, (t) => ({ ...t, status: "closed", opened: null, error: cancelled ? null : error }));
      }
    },
    disconnect: async (tabId) => {
      const connectionId = get().connByTab[tabId];
      if (!connectionId) return;
      patch(tabId, (t) => ({ ...t, status: "closing" }));
      try {
        await api.wsDisconnect(connectionId, NORMAL_CLOSE, "");
        stopCloseTimer(tabId);
        closeTimers.set(
          tabId,
          setTimeout(() => {
            closeTimers.delete(tabId);
            if (get().byTab[tabId]?.status === "closing") patch(tabId, (t) => ({ ...t, status: "closed" }));
          }, CLOSE_TIMEOUT_MS),
        );
      } catch (err) {
        patch(tabId, (t) => ({ ...t, status: t.opened ? "open" : "closed" }));
        reportError(err);
      }
    },
    send: async (tabId, location, message) => {
      const connectionId = bind(tabId, location);
      try {
        const entry = await api.wsSend(connectionId, location, message);
        patchLive(tabId, (t) => ({ ...t, log: mergeMessages(t.log, [entry]) }));
      } catch (err) {
        reportError(err);
      }
    },
    clear: async (tabId) => {
      const connectionId = get().connByTab[tabId];
      if (!connectionId) return;
      try {
        await api.wsClearLog(connectionId);
        patchLive(tabId, (t) => ({ ...t, log: [] }));
      } catch (err) {
        reportError(err);
      }
    },
    restore: async (tabId, location) => {
      const connectionId = bind(tabId, location);
      if (get().byTab[tabId]?.restored) return;
      patch(tabId, (t) => ({ ...t, restored: true }));
      try {
        const log = await api.wsLog(connectionId);
        patchLive(tabId, (t) => ({ ...t, log: mergeMessages(t.log, log) }));
      } catch {
        // A request that never connected has no log; nothing to show.
      }
    },
    setComposer: (tabId, composer) => patch(tabId, (t) => ({ ...t, composer: { ...t.composer, ...composer } })),
    apply: (event) => {
      const { kind } = event;
      const tabId = Object.keys(get().connByTab).find((id) => get().connByTab[id] === event.connection_id);
      if (tabId === undefined) return;
      if (kind.type === "closed") stopCloseTimer(tabId);
      patch(tabId, (t) => {
        if (kind.type === "opened") return { ...t, status: "open", opened: kind, error: null, closed: null };
        if (kind.type === "messages") return { ...t, log: mergeMessages(t.log, kind.items) };
        return {
          ...t,
          status: "closed",
          closed: { code: kind.code, reason: kind.reason, by: kind.by },
          error: kind.error ?? t.error,
        };
      });
    },
    release: async (tabId) => {
      const connectionId = get().connByTab[tabId];
      const status = get().byTab[tabId]?.status;
      forget(tabId);
      if (!connectionId || (status !== "open" && status !== "connecting")) return;
      try {
        await api.wsDisconnect(connectionId, NORMAL_CLOSE, "");
      } catch {
        // The connection is already gone; nothing left to release.
      }
    },
    drop: (tabId) => forget(tabId),
    reset: () => {
      listening = null;
      closeTimers.forEach((timer) => clearTimeout(timer));
      closeTimers.clear();
      set({ byTab: {}, connByTab: {}, idByLocation: {} });
    },
  };
});
