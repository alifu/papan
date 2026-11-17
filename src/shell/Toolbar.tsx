import { EnvSwitcher } from "../features/environments/EnvSwitcher";
import { Button } from "../ui/Button";
import { useLayoutStore } from "./layoutStore";
import { ThemeSwitcher } from "./ThemeSwitcher";
import { useTabsStore } from "./tabsStore";

export function Toolbar() {
  const collapsed = useLayoutStore((s) => s.sidebarCollapsed);
  const toggleSidebar = useLayoutStore((s) => s.toggleSidebar);
  const newTab = useTabsStore((s) => s.newTab);

  return (
    <div role="toolbar" aria-label="Main" className="toolbar">
      <Button icon="plus" label="New Tab" onClick={() => void newTab()} />
      <Button
        icon={collapsed ? "chevron-right" : "chevron-left"}
        aria-label="Toggle sidebar"
        pressed={!collapsed}
        onClick={toggleSidebar}
      />
      <EnvSwitcher />
      <div className="toolbar-spacer" />
      <ThemeSwitcher />
    </div>
  );
}
