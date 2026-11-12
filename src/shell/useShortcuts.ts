import { useEffect } from "react";
import { isMac } from "../ui/platform";
import { useLayoutStore } from "./layoutStore";
import { useTabsStore } from "./tabsStore";

const ACTIONS: Readonly<Record<string, () => void>> = {
  n: () => void useTabsStore.getState().newTab(),
  w: () => {
    const { activeTabId, closeTab } = useTabsStore.getState();
    if (activeTabId) closeTab(activeTabId);
  },
  b: () => useLayoutStore.getState().toggleSidebar(),
};

function handleKeyDown(event: KeyboardEvent): void {
  if (event.altKey || event.shiftKey) return;
  const mac = isMac();
  const modPressed = mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey;
  if (!modPressed) return;
  const action = ACTIONS[event.key.toLowerCase()];
  if (!action) return;
  event.preventDefault();
  action();
}

/** Mod+N new tab, Mod+W close tab, Mod+B toggle sidebar. */
export function useShortcuts(): void {
  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);
}
