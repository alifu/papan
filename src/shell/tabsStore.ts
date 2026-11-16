import { create } from "zustand";
import {
  locationWithin,
  relocate,
  sameLocation,
  type RequestLocation,
} from "../features/locations";
import { isDirty, useRequestStore } from "../features/request/requestStore";
import { useGrpcStore } from "../features/grpc/grpcStore";
import { useWsStore } from "../features/ws/wsStore";
import { useScratchStore } from "../features/scratch/scratchStore";

export type TabKind = "welcome" | "request" | "webhook" | "mock" | "journey";
export interface Tab {
  readonly id: string;
  readonly kind: TabKind;
  readonly title: string;
  readonly location?: RequestLocation;
  /** Set on `webhook` tabs: the listener the tab shows. */
  readonly webhookId?: string;
  /** Set on `mock` tabs: the mock file the tab designs. */
  readonly mock?: { readonly collectionId: string; readonly file: string };
  /** Set on `journey` tabs: the journey file the tab edits. */
  readonly journey?: { readonly collectionId: string; readonly file: string };
}

export const WELCOME_TAB: Tab = {
  id: "welcome",
  kind: "welcome",
  title: "Welcome",
};

export interface TabsState {
  readonly tabs: readonly Tab[];
  readonly activeTabId: string | null;
  readonly nextTabNumber: number;
  /** Creates a scratch request and opens it in a tab. */
  newTab: () => Promise<void>;
  /** Focuses the tab for `location` or adds one. Returns its id. */
  openRequest: (location: RequestLocation, title: string) => string;
  /** Focuses the tab for the webhook listener or adds one. Returns its id. */
  openWebhook: (webhookId: string, title: string) => string;
  /** Focuses the tab for the mock file or adds one. Returns its id. */
  openMock: (collectionId: string, file: string, title: string) => string;
  /** Focuses the tab for the journey file or adds one. Returns its id. */
  openJourney: (collectionId: string, file: string, title: string) => string;
  /** Closes the tab of a journey (it was deleted). */
  closeJourney: (collectionId: string, file: string) => void;
  /** Renames a mock tab. */
  retitleMock: (tabId: string, title: string) => void;
  /** Closes the tabs of a mock (it was deleted). */
  closeMock: (collectionId: string, file: string) => void;
  /** Renames the tab of a webhook listener. */
  retitleWebhook: (webhookId: string, title: string) => void;
  /** Closes the tab of a webhook listener (it has no draft to discard). */
  closeWebhook: (webhookId: string) => void;
  /** Points tabs at `from` (or below it) to the same place under `to`. */
  retarget: (
    from: RequestLocation,
    to: RequestLocation,
    title?: string,
  ) => void;
  /** Closes tabs at `location` or below it. */
  closeByLocation: (location: RequestLocation) => void;
  /** Closes the tab; a dirty tab first asks via `pendingCloseId`. */
  closeTab: (id: string) => void;
  /** Tab waiting for the "Discard changes?" answer. */
  pendingCloseId: string | null;
  /** Closes the tab without asking. */
  discardTab: (id: string) => void;
  cancelClose: () => void;
  activateTab: (id: string) => void;
}

function activeAfterRemoval(
  before: readonly Tab[],
  after: readonly Tab[],
  activeTabId: string | null,
): string | null {
  if (activeTabId !== null && after.some((t) => t.id === activeTabId))
    return activeTabId;
  const index = before.findIndex((t) => t.id === activeTabId);
  const neighbour = after[Math.max(index, 0)] ?? after[after.length - 1];
  return neighbour?.id ?? null;
}

export const useTabsStore = create<TabsState>()((set, get) => ({
  tabs: [WELCOME_TAB],
  activeTabId: WELCOME_TAB.id,
  nextTabNumber: 1,
  pendingCloseId: null,
  newTab: async () => {
    await useScratchStore.getState().create();
  },
  openRequest: (location, title) => {
    const { tabs, nextTabNumber } = get();
    const existing = tabs.find(
      (t) => t.location && sameLocation(t.location, location),
    );
    if (existing) {
      set({ activeTabId: existing.id });
      return existing.id;
    }
    const tab: Tab = {
      id: `tab-${nextTabNumber}`,
      kind: "request",
      title,
      location,
    };
    set({
      tabs: [...tabs, tab],
      activeTabId: tab.id,
      nextTabNumber: nextTabNumber + 1,
    });
    return tab.id;
  },
  openWebhook: (webhookId, title) => {
    const { tabs, nextTabNumber } = get();
    const existing = tabs.find((t) => t.webhookId === webhookId);
    if (existing) {
      set({ activeTabId: existing.id });
      return existing.id;
    }
    const tab: Tab = {
      id: `tab-${nextTabNumber}`,
      kind: "webhook",
      title,
      webhookId,
    };
    set({
      tabs: [...tabs, tab],
      activeTabId: tab.id,
      nextTabNumber: nextTabNumber + 1,
    });
    return tab.id;
  },
  openMock: (collectionId, file, title) => {
    const { tabs, nextTabNumber } = get();
    const existing = tabs.find(
      (t) => t.mock?.collectionId === collectionId && t.mock.file === file,
    );
    if (existing) {
      set({ activeTabId: existing.id });
      return existing.id;
    }
    const tab: Tab = {
      id: `tab-${nextTabNumber}`,
      kind: "mock",
      title,
      mock: { collectionId, file },
    };
    set({
      tabs: [...tabs, tab],
      activeTabId: tab.id,
      nextTabNumber: nextTabNumber + 1,
    });
    return tab.id;
  },
  openJourney: (collectionId, file, title) => {
    const { tabs, nextTabNumber } = get();
    const existing = tabs.find(
      (t) =>
        t.journey?.collectionId === collectionId && t.journey.file === file,
    );
    if (existing) {
      set({ activeTabId: existing.id });
      return existing.id;
    }
    const tab: Tab = {
      id: `tab-${nextTabNumber}`,
      kind: "journey",
      title,
      journey: { collectionId, file },
    };
    set({
      tabs: [...tabs, tab],
      activeTabId: tab.id,
      nextTabNumber: nextTabNumber + 1,
    });
    return tab.id;
  },
  closeJourney: (collectionId, file) => {
    const tab = get().tabs.find(
      (t) =>
        t.journey?.collectionId === collectionId && t.journey.file === file,
    );
    if (tab) get().discardTab(tab.id);
  },
  retitleMock: (tabId, title) => {
    set({
      tabs: get().tabs.map((t) => (t.id === tabId ? { ...t, title } : t)),
    });
  },
  closeMock: (collectionId, file) => {
    const tab = get().tabs.find(
      (t) => t.mock?.collectionId === collectionId && t.mock.file === file,
    );
    if (tab) get().discardTab(tab.id);
  },
  retitleWebhook: (webhookId, title) => {
    set({
      tabs: get().tabs.map((t) =>
        t.webhookId === webhookId ? { ...t, title } : t,
      ),
    });
  },
  closeWebhook: (webhookId) => {
    const tab = get().tabs.find((t) => t.webhookId === webhookId);
    if (tab) get().discardTab(tab.id);
  },
  retarget: (from, to, title) => {
    set({
      tabs: get().tabs.map((tab) => {
        const next = tab.location ? relocate(from, to, tab.location) : null;
        if (!next) return tab;
        const exact =
          tab.location !== undefined && sameLocation(tab.location, from);
        return {
          ...tab,
          location: next,
          title: exact && title !== undefined ? title : tab.title,
        };
      }),
    });
  },
  closeByLocation: (location) => {
    const { tabs, activeTabId } = get();
    const remaining = tabs.filter(
      (t) => !(t.location && locationWithin(location, t.location)),
    );
    if (remaining.length === tabs.length) return;
    tabs
      .filter((t) => !remaining.includes(t))
      .forEach((t) => {
        useRequestStore.getState().drop(t.id);
        void useWsStore.getState().release(t.id);
        void useGrpcStore.getState().release(t.id);
      });
    set({
      tabs: remaining,
      activeTabId: activeAfterRemoval(tabs, remaining, activeTabId),
    });
  },
  closeTab: (id) => {
    if (!get().tabs.some((t) => t.id === id)) return;
    if (isDirty(id)) set({ pendingCloseId: id });
    else get().discardTab(id);
  },
  discardTab: (id) => {
    const { tabs, activeTabId, pendingCloseId } = get();
    if (!tabs.some((t) => t.id === id)) return;
    useRequestStore.getState().drop(id);
    void useWsStore.getState().release(id);
    void useGrpcStore.getState().release(id);
    const remaining = tabs.filter((t) => t.id !== id);
    set({
      tabs: remaining,
      activeTabId: activeAfterRemoval(tabs, remaining, activeTabId),
      pendingCloseId: pendingCloseId === id ? null : pendingCloseId,
    });
  },
  cancelClose: () => set({ pendingCloseId: null }),
  activateTab: (id) => {
    if (get().tabs.some((t) => t.id === id)) set({ activeTabId: id });
  },
}));
