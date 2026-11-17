import { useRequestStore } from "../features/request/requestStore";
import type { RequestLocation } from "../ipc/bindings";
import { useTabsStore } from "./tabsStore";

function withActiveRequest(
  run: (tabId: string, location: RequestLocation) => Promise<void>,
): void {
  const { tabs, activeTabId } = useTabsStore.getState();
  const tab = tabs.find((t) => t.id === activeTabId);
  if (tab?.location) void run(tab.id, tab.location);
}

export const saveActiveRequest = (): void =>
  withActiveRequest((id, loc) => useRequestStore.getState().save(id, loc));
export const sendActiveRequest = (): void =>
  withActiveRequest((id, loc) => useRequestStore.getState().send(id, loc));
