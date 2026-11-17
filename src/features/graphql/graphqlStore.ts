import { create } from "zustand";
import type { GraphqlSchema, RequestContent, RequestLocation } from "../../ipc/bindings";
import { isAppError } from "../../ipc/errors";
import { reportError } from "../../ui/toastStore";
import * as api from "./api";

export type SchemaStatus = "idle" | "loading" | "ready" | "error";

/** Per-tab schema state. */
export interface GraphqlTabState {
  readonly status: SchemaStatus;
  readonly schema: GraphqlSchema | null;
  readonly endpoint: string;
  readonly fetchedAt: number | null;
  readonly error: string | null;
  /** The introspection in flight, if any. */
  readonly requestId: string | null;
}

const EMPTY: GraphqlTabState = { status: "idle", schema: null, endpoint: "", fetchedAt: null, error: null, requestId: null };

export interface GraphqlState {
  readonly byTab: Readonly<Record<string, GraphqlTabState>>;
  /** Reads the cached schema (no network); a miss leaves the tab idle. Failures are silent. */
  loadCached: (tabId: string, location: RequestLocation, content: RequestContent) => Promise<void>;
  introspect: (tabId: string, location: RequestLocation, content: RequestContent) => Promise<void>;
  /** Picks a schema file (introspection JSON or SDL); a cancelled dialog changes nothing. */
  loadFile: (tabId: string, location: RequestLocation, content: RequestContent) => Promise<void>;
  clear: (tabId: string, location: RequestLocation, content: RequestContent) => Promise<void>;
  cancel: (tabId: string) => void;
  drop: (tabId: string) => void;
}

export const graphqlTab = (s: GraphqlState, tabId: string): GraphqlTabState => s.byTab[tabId] ?? EMPTY;

const ready = (schema: GraphqlSchema): Partial<GraphqlTabState> => ({
  status: "ready",
  schema,
  endpoint: schema.endpoint,
  fetchedAt: schema.fetched_at_ms,
  error: null,
  requestId: null,
});

const messageOf = (err: unknown): string => (isAppError(err) ? err.message : "The schema could not be loaded.");

export const useGraphqlStore = create<GraphqlState>()((set, get) => {
  const patch = (tabId: string, change: Partial<GraphqlTabState>) =>
    set((s) => ({ byTab: { ...s.byTab, [tabId]: { ...graphqlTab(s, tabId), ...change } } }));
  // Only the newest call per tab may write its result.
  const tickets = new Map<string, number>();
  const nextTicket = (tabId: string): (() => boolean) => {
    const ticket = (tickets.get(tabId) ?? 0) + 1;
    tickets.set(tabId, ticket);
    return () => tickets.get(tabId) === ticket;
  };

  return {
    byTab: {},
    loadCached: async (tabId, location, content) => {
      const current = nextTicket(tabId);
      try {
        const schema = await api.readCachedSchema(location, content);
        if (current() && schema) patch(tabId, ready(schema));
      } catch {
        // A half-typed URL has no cache; nothing to show.
      }
    },
    introspect: async (tabId, location, content) => {
      if (graphqlTab(get(), tabId).requestId !== null) return;
      const current = nextTicket(tabId);
      const requestId = crypto.randomUUID();
      patch(tabId, { status: "loading", error: null, requestId });
      try {
        const schema = await api.introspectGraphql(requestId, location, content);
        if (current()) patch(tabId, ready(schema));
      } catch (err) {
        if (!current()) return;
        const cancelled = isAppError(err) && err.code === "cancelled";
        const had = graphqlTab(get(), tabId).schema !== null;
        patch(tabId, {
          status: cancelled ? (had ? "ready" : "idle") : "error",
          error: cancelled ? null : messageOf(err),
          requestId: null,
        });
      }
    },
    loadFile: async (tabId, location, content) => {
      const current = nextTicket(tabId);
      try {
        const schema = await api.loadSchemaFile(location, content);
        if (current() && schema) patch(tabId, ready(schema));
      } catch (err) {
        if (current()) patch(tabId, { status: "error", error: messageOf(err) });
      }
    },
    clear: async (tabId, location, content) => {
      try {
        await api.clearCachedSchema(location, content);
        nextTicket(tabId);
        patch(tabId, EMPTY);
      } catch (err) {
        reportError(err);
      }
    },
    cancel: (tabId) => {
      const { requestId } = graphqlTab(get(), tabId);
      if (requestId !== null) void api.cancelIntrospection(requestId).catch(reportError);
    },
    drop: (tabId) => {
      get().cancel(tabId);
      tickets.delete(tabId);
      set((s) => {
        const rest = { ...s.byTab };
        delete rest[tabId];
        return { byTab: rest };
      });
    },
  };
});
