import type { RequestLocation } from "../../ipc/bindings";
import { useTabsStore } from "../../shell/tabsStore";

/** Location of the active request tab, or null (Welcome tab, no tabs). */
export function useActiveLocation(): RequestLocation | null {
  return useTabsStore((s) => s.tabs.find((t) => t.id === s.activeTabId)?.location ?? null);
}
