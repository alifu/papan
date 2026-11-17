import { create } from "zustand";
import type { OAuthStatus, RequestContent, RequestLocation } from "../../ipc/bindings";
import { isAppError } from "../../ipc/errors";
import { reportError } from "../../ui/toastStore";
import * as api from "./oauthApi";

export const CANCELLED_TEXT = "Cancelled";

/** Per-tab token state. Holds `OAuthStatus` only: no token ever reaches the webview. */
export interface OAuthTabState {
  readonly status: OAuthStatus | null;
  /** The flow in progress, if any. */
  readonly flowId: string | null;
  /** Short note from the last flow ("Cancelled"). */
  readonly notice: string | null;
  /** The last background status read failed (typically a half-typed configuration). */
  readonly incomplete: boolean;
}

const EMPTY: OAuthTabState = { status: null, flowId: null, notice: null, incomplete: false };

export interface OAuthState {
  readonly byTab: Readonly<Record<string, OAuthTabState>>;
  /** Reads the status. Background reads are silent (the panel shows "Configuration incomplete"); `report` toasts the error. */
  refresh: (tabId: string, location: RequestLocation, content: RequestContent, report?: boolean) => Promise<OAuthStatus | null>;
  authorize: (tabId: string, location: RequestLocation, content: RequestContent, configKey: string) => Promise<void>;
  cancel: (tabId: string) => void;
  clear: (tabId: string, location: RequestLocation, content: RequestContent) => Promise<void>;
  /** The tab closed: cancel its pending flow and forget its state. */
  drop: (tabId: string) => void;
}

export const tabState = (s: OAuthState, tabId: string): OAuthTabState => s.byTab[tabId] ?? EMPTY;

export const useOAuthStore = create<OAuthState>()((set, get) => {
  const patch = (tabId: string, change: Partial<OAuthTabState>) =>
    set((s) => ({ byTab: { ...s.byTab, [tabId]: { ...tabState(s, tabId), ...change } } }));
  // A status answer is applied only if no newer status call for the tab started since.
  const tickets = new Map<string, number>();
  const nextTicket = (tabId: string): number => {
    const ticket = (tickets.get(tabId) ?? 0) + 1;
    tickets.set(tabId, ticket);
    return ticket;
  };

  return {
    byTab: {},
    refresh: async (tabId, location, content, report = false) => {
      const ticket = nextTicket(tabId);
      try {
        const status = await api.oauthStatus(location, content);
        if (tickets.get(tabId) === ticket) patch(tabId, { status, incomplete: false });
        return status;
      } catch (err) {
        if (tickets.get(tabId) === ticket) patch(tabId, { status: null, incomplete: true });
        if (report) reportError(err);
        return null;
      }
    },
    authorize: async (tabId, location, content, configKey) => {
      if (tabState(get(), tabId).flowId !== null) return;
      const flowId = crypto.randomUUID();
      patch(tabId, { flowId, notice: null });
      try {
        const status = await api.oauthAuthorize(flowId, location, content, configKey);
        nextTicket(tabId);
        patch(tabId, { status, flowId: null, incomplete: false });
      } catch (err) {
        patch(tabId, { flowId: null, notice: isAppError(err) && err.code === "cancelled" ? CANCELLED_TEXT : null });
        if (!(isAppError(err) && err.code === "cancelled")) reportError(err);
      }
    },
    cancel: (tabId) => {
      const { flowId } = tabState(get(), tabId);
      if (flowId === null) return;
      api.oauthCancel(flowId).catch(reportError);
    },
    clear: async (tabId, location, content) => {
      try {
        const status = await api.oauthClear(location, content);
        nextTicket(tabId);
        patch(tabId, { status, notice: null });
      } catch (err) {
        reportError(err);
      }
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
