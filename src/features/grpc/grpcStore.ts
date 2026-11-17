import { create } from "zustand";
import type {
  AppError,
  GrpcEndBy,
  GrpcEvent,
  GrpcSchema,
  GrpcStarted,
  GrpcStatus,
  KeyValue,
  RequestContent,
  RequestLocation,
  WsMessage,
} from "../../ipc/bindings";
import { isAppError } from "../../ipc/errors";
import { reportError } from "../../ui/toastStore";
import { mergeMessages } from "../ws/wsModel";
import * as api from "./api";
import type { CallStatus } from "./grpcModel";

export interface GrpcTab {
  readonly status: CallStatus;
  /** Events older than this belong to a previous run of the same call id. */
  readonly attempt: number;
  readonly minAttempt: number;
  readonly started: GrpcStarted | null;
  readonly headers: readonly KeyValue[] | null;
  readonly log: readonly WsMessage[];
  readonly grpcStatus: GrpcStatus | null;
  readonly trailers: readonly KeyValue[];
  readonly by: GrpcEndBy | null;
  readonly elapsedMs: number | null;
  readonly error: AppError | null;
  /** The backend log was fetched for this tab (re-opened tab). */
  readonly restored: boolean;
  readonly schema: GrpcSchema | null;
  readonly schemaError: AppError | null;
  readonly schemaLoading: boolean;
  /** Reflection has no cache and auto-load must not touch the network: the user has to ask. */
  readonly schemaNeedsRefresh: boolean;
}

export interface GrpcState {
  /** UI state keyed by tab id. */
  readonly byTab: Readonly<Record<string, GrpcTab>>;
  /** The call id of each open tab. */
  readonly callByTab: Readonly<Record<string, string>>;
  /** Stable UUID call id per request location, so a re-opened request finds its log. */
  readonly idByLocation: Readonly<Record<string, string>>;
  start: (tabId: string, location: RequestLocation, content: RequestContent) => Promise<void>;
  send: (tabId: string, location: RequestLocation, message: string) => Promise<void>;
  /** Half-closes a client or bidi stream. */
  end: (tabId: string) => Promise<void>;
  /** Cancels connect or call; the tab stays closing until the finished event. */
  cancel: (tabId: string) => Promise<void>;
  clear: (tabId: string) => Promise<void>;
  restore: (tabId: string, location: RequestLocation) => Promise<void>;
  loadSchema: (tabId: string, location: RequestLocation, content: RequestContent) => Promise<void>;
  /** Cache and proto files only; never touches the network (runs unprompted). */
  /** Explicit reflection fetch: sends the request's metadata to the server. */
  reflect: (tabId: string, location: RequestLocation, content: RequestContent) => Promise<void>;
  apply: (event: GrpcEvent) => void;
  /** Cancels the tab's call if live and forgets its state. */
  release: (tabId: string) => Promise<void>;
  drop: (tabId: string) => void;
  reset: () => void;
}

export const CANCEL_TIMEOUT_MS = 5000;
const LIVE: readonly CallStatus[] = ["connecting", "open", "half_closed", "closing"];
const locationKey = (location: RequestLocation): string => JSON.stringify(location);
const INTERNAL_ERROR: AppError = { code: "internal", message: "Something went wrong. Please try again.", detail: null };
const toAppError = (err: unknown): AppError => (isAppError(err) ? err : INTERNAL_ERROR);

const EMPTY_TAB: GrpcTab = {
  status: "idle",
  attempt: 0,
  minAttempt: 0,
  started: null,
  headers: null,
  log: [],
  grpcStatus: null,
  trailers: [],
  by: null,
  elapsedMs: null,
  error: null,
  restored: false,
  schema: null,
  schemaError: null,
  schemaLoading: false,
  schemaNeedsRefresh: false,
};

let listening: Promise<unknown> | null = null;
const cancelTimers = new Map<string, ReturnType<typeof setTimeout>>();
const schemaRuns = new Map<string, number>();
const stopCancelTimer = (tabId: string) => {
  clearTimeout(cancelTimers.get(tabId));
  cancelTimers.delete(tabId);
};

export const useGrpcStore = create<GrpcState>()((set, get) => {
  const patch = (tabId: string, update: (tab: GrpcTab) => GrpcTab) =>
    set((s) => ({ byTab: { ...s.byTab, [tabId]: update(s.byTab[tabId] ?? EMPTY_TAB) } }));
  /** Like `patch`, but never recreates a tab that was released meanwhile. */
  const patchLive = (tabId: string, update: (tab: GrpcTab) => GrpcTab) => {
    if (get().byTab[tabId]) patch(tabId, update);
  };
  const bind = (tabId: string, location: RequestLocation): string => {
    const key = locationKey(location);
    const id = get().idByLocation[key] ?? crypto.randomUUID();
    set((s) => ({
      idByLocation: { ...s.idByLocation, [key]: id },
      callByTab: { ...s.callByTab, [tabId]: id },
    }));
    return id;
  };
  const ensureListening = () => {
    listening ??= api.listenGrpcEvents((event) => get().apply(event)).catch((err: unknown) => {
      listening = null;
      throw err;
    });
    return listening;
  };
  const forget = (tabId: string) => {
    stopCancelTimer(tabId);
    schemaRuns.delete(tabId);
    set((s) => {
      const byTab = { ...s.byTab };
      const callByTab = { ...s.callByTab };
      delete byTab[tabId];
      delete callByTab[tabId];
      return { byTab, callByTab };
    });
  };
  const applyTo = (tabId: string, event: GrpcEvent) => {
    const { kind } = event;
    const current = get().byTab[tabId];
    if (!current || event.attempt < current.minAttempt) return;
    if (kind.type === "finished") stopCancelTimer(tabId);
    patch(tabId, (t) => {
      const next = { ...t, attempt: Math.max(t.attempt, event.attempt) };
      if (kind.type === "headers") return { ...next, headers: kind.metadata };
      if (kind.type === "messages") return { ...next, log: mergeMessages(t.log, kind.items) };
      return {
        ...next,
        status: "finished",
        grpcStatus: kind.status,
        trailers: kind.trailers,
        by: kind.by,
        elapsedMs: kind.elapsed_ms,
        error: kind.error,
      };
    });
  };
  const loadWith = async (tabId: string, fetch: () => Promise<GrpcSchema | null>) => {
    const run = (schemaRuns.get(tabId) ?? 0) + 1;
    schemaRuns.set(tabId, run);
    patch(tabId, (t) => ({ ...t, schemaLoading: true }));
    try {
      const schema = await fetch();
      if (schemaRuns.get(tabId) === run) patchLive(tabId, (t) => ({ ...t, schema, schemaError: null, schemaLoading: false, schemaNeedsRefresh: schema === null }));
    } catch (err) {
      if (schemaRuns.get(tabId) === run) {
        patchLive(tabId, (t) => ({ ...t, schema: null, schemaError: toAppError(err), schemaLoading: false, schemaNeedsRefresh: false }));
      }
    }
  };

  return {
    byTab: {},
    callByTab: {},
    idByLocation: {},
    start: async (tabId, location, content) => {
      const callId = bind(tabId, location);
      stopCancelTimer(tabId);
      patch(tabId, (t) => ({
        ...t,
        status: "connecting",
        minAttempt: t.attempt + 1,
        started: null,
        headers: null,
        // The engine numbers messages from 1 on every attempt, so the old log would hide the new ones.
        log: [],
        grpcStatus: null,
        trailers: [],
        by: null,
        elapsedMs: null,
        error: null,
      }));
      try {
        await ensureListening();
        const info = await api.grpcStart(callId, location, content);
        patchLive(tabId, (t) => ({
          ...t,
          attempt: Math.max(t.attempt, info.attempt),
          minAttempt: info.attempt,
          started: info,
          status: t.status === "connecting" ? "open" : t.status,
        }));
      } catch (err) {
        if (!get().byTab[tabId]) return;
        const error = toAppError(err);
        patchLive(tabId, (t) =>
          error.code === "cancelled" ? { ...t, status: "idle", error: null } : { ...t, status: "finished", error },
        );
      }
    },
    send: async (tabId, location, message) => {
      const callId = bind(tabId, location);
      try {
        const entry = await api.grpcSend(callId, location, message);
        patchLive(tabId, (t) => ({ ...t, log: mergeMessages(t.log, [entry]) }));
      } catch (err) {
        reportError(err);
      }
    },
    end: async (tabId) => {
      const callId = get().callByTab[tabId];
      if (!callId) return;
      try {
        await api.grpcEnd(callId);
        patchLive(tabId, (t) => (t.status === "open" ? { ...t, status: "half_closed" } : t));
      } catch (err) {
        reportError(err);
      }
    },
    cancel: async (tabId) => {
      const callId = get().callByTab[tabId];
      const before = get().byTab[tabId]?.status;
      if (!callId || before === undefined || !LIVE.includes(before)) return;
      patch(tabId, (t) => ({ ...t, status: "closing" }));
      try {
        await api.grpcCancel(callId);
        stopCancelTimer(tabId);
        cancelTimers.set(
          tabId,
          setTimeout(() => {
            cancelTimers.delete(tabId);
            if (get().byTab[tabId]?.status === "closing") patch(tabId, (t) => ({ ...t, status: "finished" }));
          }, CANCEL_TIMEOUT_MS),
        );
      } catch (err) {
        patchLive(tabId, (t) => ({ ...t, status: before }));
        reportError(err);
      }
    },
    clear: async (tabId) => {
      const callId = get().callByTab[tabId];
      if (!callId) return;
      try {
        await api.grpcClearLog(callId);
        patchLive(tabId, (t) => ({ ...t, log: [] }));
      } catch (err) {
        reportError(err);
      }
    },
    restore: async (tabId, location) => {
      const callId = bind(tabId, location);
      if (get().byTab[tabId]?.restored) return;
      patch(tabId, (t) => ({ ...t, restored: true }));
      try {
        const log = await api.grpcLog(callId);
        patchLive(tabId, (t) => ({ ...t, log: mergeMessages(t.log, log) }));
      } catch {
        // A request that never ran has no log; nothing to show.
      }
    },
    loadSchema: (tabId, location, content) => loadWith(tabId, () => api.grpcSchema(location, content, false)),
    reflect: (tabId, location, content) => loadWith(tabId, () => api.grpcReflect(location, content, true)),
    apply: (event) => {
      const tabIds = Object.keys(get().callByTab).filter((id) => get().callByTab[id] === event.call_id);
      for (const tabId of tabIds) applyTo(tabId, event);
    },
    release: async (tabId) => {
      const callId = get().callByTab[tabId];
      const status = get().byTab[tabId]?.status;
      forget(tabId);
      if (!callId || status === undefined || !LIVE.includes(status)) return;
      try {
        await api.grpcCancel(callId);
      } catch {
        // The call is already gone; nothing left to release.
      }
    },
    drop: (tabId) => forget(tabId),
    reset: () => {
      const pending = listening;
      listening = null;
      void pending?.then((unlisten) => (typeof unlisten === "function" ? unlisten() : undefined)).catch(() => {});
      cancelTimers.forEach((timer) => clearTimeout(timer));
      cancelTimers.clear();
      schemaRuns.clear();
      set({ byTab: {}, callByTab: {}, idByLocation: {} });
    },
  };
});
