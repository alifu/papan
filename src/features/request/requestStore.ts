import { create } from "zustand";
import type {
  AppError,
  RequestContent,
  RequestDoc,
  RequestLocation,
  ResponseMeta,
} from "../../ipc/bindings";
import { isAppError } from "../../ipc/errors";
import { reportError } from "../../ui/toastStore";
import { readRequest, writeRequest } from "../collections/api";
import * as api from "./api";
import { equalContent } from "./content";
import { applyRowsEdit, applyUrlEdit, rowsFromContent, type ParamRow } from "./params";

export interface TabRequest {
  readonly doc: RequestDoc | null;
  /** Unsaved content; `null` when equal to `doc.content` (so `dirty = draft !== null`). */
  readonly draft: RequestContent | null;
  readonly missing: boolean;
  readonly sending: boolean;
  readonly requestId: string | null;
  readonly response: ResponseMeta | null;
  readonly error: AppError | null;
  /** Session-only view of the params table (keeps disabled rows in place). */
  readonly paramRows: readonly ParamRow[];
}

export interface RequestState {
  readonly byTab: Readonly<Record<string, TabRequest>>;
  load: (tabId: string, location: RequestLocation) => Promise<void>;
  /** Replaces the draft with `update(current content)`. */
  edit: (tabId: string, update: (content: RequestContent) => RequestContent) => void;
  editUrl: (tabId: string, url: string) => void;
  editParams: (tabId: string, rows: readonly ParamRow[]) => void;
  save: (tabId: string, location: RequestLocation) => Promise<void>;
  send: (tabId: string, location: RequestLocation) => Promise<void>;
  cancel: (tabId: string) => void;
  /** Forgets the tab's state and cancels its in-flight request. */
  drop: (tabId: string) => void;
}

const EMPTY_TAB: TabRequest = {
  doc: null,
  draft: null,
  missing: false,
  sending: false,
  requestId: null,
  response: null,
  error: null,
  paramRows: [],
};

const INTERNAL_ERROR: AppError = { code: "internal", message: "Something went wrong. Please try again.", detail: null };

export const currentContent = (tab: TabRequest | undefined): RequestContent | null =>
  tab?.draft ?? tab?.doc?.content ?? null;

export const isDirty = (tabId: string): boolean =>
  useRequestStore.getState().byTab[tabId]?.draft != null;

export const useRequestStore = create<RequestState>()((set, get) => {
  const tabOf = (tabId: string): TabRequest => get().byTab[tabId] ?? EMPTY_TAB;
  const patch = (tabId: string, change: Partial<TabRequest>) =>
    set((state) => ({ byTab: { ...state.byTab, [tabId]: { ...(state.byTab[tabId] ?? EMPTY_TAB), ...change } } }));

  const withDraft = (tabId: string, next: RequestContent, extra: Partial<TabRequest> = {}) => {
    const doc = tabOf(tabId).doc;
    if (!doc) return;
    patch(tabId, { ...extra, draft: equalContent(next, doc.content) ? null : next });
  };

  return {
    byTab: {},
    load: async (tabId, location) => {
      try {
        const doc = await readRequest(location);
        const tab = tabOf(tabId);
        const clean = tab.draft === null;
        patch(tabId, {
          doc,
          missing: false,
          ...(clean ? { paramRows: rowsFromContent(doc.content.url, doc.content.disabled_params) } : {}),
        });
      } catch (err) {
        if (isAppError(err) && err.code === "not_found") patch(tabId, { missing: true });
        else reportError(err);
      }
    },
    edit: (tabId, update) => {
      const current = currentContent(get().byTab[tabId]);
      if (current) withDraft(tabId, update(current));
    },
    editUrl: (tabId, url) => {
      const current = currentContent(get().byTab[tabId]);
      if (!current) return;
      withDraft(tabId, { ...current, url }, { paramRows: applyUrlEdit(tabOf(tabId).paramRows, url) });
    },
    editParams: (tabId, rows) => {
      const current = currentContent(get().byTab[tabId]);
      if (!current) return;
      const { url, disabled_params } = applyRowsEdit(current.url, rows);
      withDraft(tabId, { ...current, url, disabled_params: [...disabled_params] }, { paramRows: rows });
    },
    save: async (tabId, location) => {
      const { doc, draft } = tabOf(tabId);
      if (!doc || draft === null) return;
      try {
        await writeRequest(location, draft);
        const latest = tabOf(tabId);
        const stillSame = latest.draft !== null && equalContent(latest.draft, draft);
        patch(tabId, { doc: { ...doc, content: draft }, draft: stillSame ? null : latest.draft });
      } catch (err) {
        reportError(err);
      }
    },
    send: async (tabId, location) => {
      const content = currentContent(get().byTab[tabId]);
      if (!content || tabOf(tabId).sending) return;
      const requestId = crypto.randomUUID();
      patch(tabId, { sending: true, requestId, error: null });
      const superseded = () => tabOf(tabId).requestId !== requestId;
      try {
        const response = await api.sendRequest(requestId, location, content);
        if (superseded()) return;
        patch(tabId, { response, sending: false });
      } catch (err) {
        if (superseded()) return;
        patch(tabId, { sending: false, response: null, error: isAppError(err) ? err : INTERNAL_ERROR });
      }
    },
    cancel: (tabId) => {
      const { requestId, sending } = tabOf(tabId);
      if (requestId && sending) void api.cancelRequest(requestId).catch(reportError);
    },
    drop: (tabId) => {
      get().cancel(tabId);
      set((state) => {
        const rest = { ...state.byTab };
        delete rest[tabId];
        return { byTab: rest };
      });
    },
  };
});
