import { useEffect } from "react";
import { useCollectionsStore } from "../features/collections/collectionsStore";
import { useScratchStore } from "../features/scratch/scratchStore";
import { ToastHost } from "../ui/Toast";
import { MenuBar } from "./MenuBar";
import { Sidebar } from "./Sidebar";
import { StatusBar } from "./StatusBar";
import { TabPanel } from "./TabPanel";
import { TabStrip } from "./TabStrip";
import { Toolbar } from "./Toolbar";
import {
  SIDEBAR_KEY_STEP,
  SIDEBAR_MAX_WIDTH,
  SIDEBAR_MIN_WIDTH,
  useLayoutStore,
} from "./layoutStore";
import { useShortcuts } from "./useShortcuts";
import { Splitter } from "../ui/Splitter";

export function AppShell() {
  useShortcuts();
  useEffect(() => {
    void useCollectionsStore.getState().load();
    void useScratchStore.getState().load();
    const unlisten = useCollectionsStore.getState().subscribe();
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);
  const width = useLayoutStore((s) => s.sidebarWidth);
  const collapsed = useLayoutStore((s) => s.sidebarCollapsed);
  const setWidth = useLayoutStore((s) => s.setSidebarWidth);
  const toggle = useLayoutStore((s) => s.toggleSidebar);

  const columns = collapsed
    ? "var(--rail-width) 1fr"
    : `${width}px var(--splitter-size) 1fr`;

  return (
    <div className="shell">
      <MenuBar />
      <Toolbar />
      <div className="shell-body" style={{ gridTemplateColumns: columns }}>
        <Sidebar />
        {collapsed ? null : (
          <Splitter
            value={width}
            min={SIDEBAR_MIN_WIDTH}
            max={SIDEBAR_MAX_WIDTH}
            step={SIDEBAR_KEY_STEP}
            label="Resize sidebar"
            onResize={setWidth}
            onToggle={toggle}
          />
        )}
        <main className="workspace">
          <TabStrip />
          <TabPanel />
        </main>
      </div>
      <StatusBar />
      <ToastHost />
    </div>
  );
}
