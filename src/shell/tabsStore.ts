import { create } from "zustand";
import {
  locationWithin,
  relocate,
  sameLocation,
  type RequestLocation,
} from "../features/locations";
import { isDirty, useRequestStore } from "../features/request/requestStore";
import { useScratchStore } from "../features/scratch/scratchStore";

export type TabKind = "welcome" | "request";
export interface Tab {
  readonly id: string;
  readonly kind: TabKind;
  readonly title: string;
  readonly location?: RequestLocation;
}

export const WELCOME_TAB: Tab = { id: "welcome", kind: "welcome", title: "Welcome" };

export interface TabsState {
  readonly tabs: readonly Tab[];
  readonly activeTabId: string | null;
  readonly nextTabNumber: number;
  /** Creates a scratch request and opens it in a tab. */
  newTab: () => Promise<void>;
  /** Focuses the tab for `location` or adds one. Returns its id. */
  openRequest: (location: RequestLocation, title: string) => string;
  /** Points tabs at `from` (or below it) to the same place under `to`. */
  retarget: (from: RequestLocation, to: RequestLocation, title?: string) => void;
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
  if (activeTabId !== null && after.some((t) => t.id === activeTabId)) return activeTabId;
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
    const existing = tabs.find((t) => t.location && sameLocation(t.location, location));
    if (existing) {
      set({ activeTabId: existing.id });
      return existing.id;
    }
    const tab: Tab = { id: `tab-${nextTabNumber}`, kind: "request", title, location };
    set({ tabs: [...tabs, tab], activeTabId: tab.id, nextTabNumber: nextTabNumber + 1 });
    return tab.id;
  },
  retarget: (from, to, title) => {
    set({
      tabs: get().tabs.map((tab) => {
        const next = tab.location ? relocate(from, to, tab.location) : null;
        if (!next) return tab;
        const exact = tab.location !== undefined && sameLocation(tab.location, from);
        return { ...tab, location: next, title: exact && title !== undefined ? title : tab.title };
      }),
    });
  },
  closeByLocation: (location) => {
    const { tabs, activeTabId } = get();
    const remaining = tabs.filter((t) => !(t.location && locationWithin(location, t.location)));
    if (remaining.length === tabs.length) return;
    tabs.filter((t) => !remaining.includes(t)).forEach((t) => useRequestStore.getState().drop(t.id));
    set({ tabs: remaining, activeTabId: activeAfterRemoval(tabs, remaining, activeTabId) });
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
