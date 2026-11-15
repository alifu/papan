import { useEffect } from "react";
import { isMac } from "../ui/platform";
import { copyAsCurl } from "../features/curl/copyAsCurl";
import { useLayoutStore } from "./layoutStore";
import { saveActiveRequest, sendActiveRequest } from "./activeRequest";
import { useTabsStore } from "./tabsStore";

const ACTIONS: Readonly<Record<string, () => void>> = {
  n: () => void useTabsStore.getState().newTab(),
  w: () => {
    const { activeTabId, closeTab } = useTabsStore.getState();
    if (activeTabId) closeTab(activeTabId);
  },
  b: () => useLayoutStore.getState().toggleSidebar(),
  s: saveActiveRequest,
  enter: sendActiveRequest,
};

const SHIFT_ACTIONS: Readonly<Record<string, () => void>> = {
  c: () => void copyAsCurl(),
};

function handleKeyDown(event: KeyboardEvent): void {
  if (event.altKey) return;
  const mac = isMac();
  const modPressed = mac
    ? event.metaKey && !event.ctrlKey
    : event.ctrlKey && !event.metaKey;
  if (!modPressed) return;
  const action = (event.shiftKey ? SHIFT_ACTIONS : ACTIONS)[
    event.key.toLowerCase()
  ];
  if (!action) return;
  event.preventDefault();
  action();
}

/** Mod+N new tab, Mod+W close tab, Mod+B toggle sidebar, Mod+S save, Mod+Enter send, Mod+Shift+C copy as cURL. */
export function useShortcuts(): void {
  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);
}
