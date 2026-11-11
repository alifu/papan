import { Menu } from "../ui/Menu";
import { shortcutLabel } from "../ui/platform";
import { useLayoutStore } from "./layoutStore";
import { useTabsStore } from "./tabsStore";

export function MenuBar() {
  const newTab = useTabsStore((s) => s.newTab);
  const closeTab = useTabsStore((s) => s.closeTab);
  const toggleSidebar = useLayoutStore((s) => s.toggleSidebar);

  const closeActiveTab = () => {
    const { activeTabId } = useTabsStore.getState();
    if (activeTabId) closeTab(activeTabId);
  };

  return (
    <div role="menubar" className="menubar">
      <Menu
        label="File"
        items={[
          { label: "New Tab", shortcut: shortcutLabel("N"), onSelect: newTab },
          { label: "Close Tab", shortcut: shortcutLabel("W"), onSelect: closeActiveTab },
        ]}
      />
      <Menu
        label="View"
        items={[{ label: "Toggle Sidebar", shortcut: shortcutLabel("B"), onSelect: toggleSidebar }]}
      />
    </div>
  );
}
