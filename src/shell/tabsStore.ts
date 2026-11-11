import { create } from "zustand";

export type TabKind = "welcome" | "blank";
export interface Tab {
  readonly id: string;
  readonly kind: TabKind;
  readonly title: string;
}

export const WELCOME_TAB: Tab = { id: "welcome", kind: "welcome", title: "Welcome" };

export interface TabsState {
  readonly tabs: readonly Tab[];
  readonly activeTabId: string | null;
  readonly nextTabNumber: number;
  newTab: () => string;
  closeTab: (id: string) => void;
  activateTab: (id: string) => void;
}

function nextActiveAfterClose(
  tabs: readonly Tab[],
  closedIndex: number,
): string | null {
  const remaining = tabs.filter((_, i) => i !== closedIndex);
  const neighbour = remaining[closedIndex] ?? remaining[remaining.length - 1];
  return neighbour?.id ?? null;
}

export const useTabsStore = create<TabsState>()((set, get) => ({
  tabs: [WELCOME_TAB],
  activeTabId: WELCOME_TAB.id,
  nextTabNumber: 1,
  newTab: () => {
    const { tabs, nextTabNumber } = get();
    const tab: Tab = {
      id: `tab-${nextTabNumber}`,
      kind: "blank",
      title: `Untitled ${nextTabNumber}`,
    };
    set({ tabs: [...tabs, tab], activeTabId: tab.id, nextTabNumber: nextTabNumber + 1 });
    return tab.id;
  },
  closeTab: (id) => {
    const { tabs, activeTabId } = get();
    const index = tabs.findIndex((t) => t.id === id);
    if (index < 0) return;
    set({
      tabs: tabs.filter((t) => t.id !== id),
      activeTabId: id === activeTabId ? nextActiveAfterClose(tabs, index) : activeTabId,
    });
  },
  activateTab: (id) => {
    if (get().tabs.some((t) => t.id === id)) set({ activeTabId: id });
  },
}));
