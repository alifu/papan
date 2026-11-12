import { copyAsCurl } from "../features/curl/copyAsCurl";
import { Menu } from "../ui/Menu";
import { shortcutLabel } from "../ui/platform";
import { saveActiveRequest, sendActiveRequest } from "./activeRequest";
import { useDialogsStore } from "./dialogsStore";
import { useLayoutStore } from "./layoutStore";
import { useTabsStore } from "./tabsStore";

export function MenuBar() {
  const newTab = useTabsStore((s) => s.newTab);
  const closeTab = useTabsStore((s) => s.closeTab);
  const showDialog = useDialogsStore((s) => s.show);
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
          { label: "Save", shortcut: shortcutLabel("S"), onSelect: saveActiveRequest },
          { label: "Import cURL…", onSelect: () => showDialog("import-curl") },
          { label: "Close Tab", shortcut: shortcutLabel("W"), onSelect: closeActiveTab },
        ]}
      />
      <Menu label="Edit" items={[{ label: "Network settings…", onSelect: () => showDialog("network") }]} />
      <Menu
        label="Request"
        items={[
          { label: "Send", shortcut: shortcutLabel("Enter"), onSelect: sendActiveRequest },
          { label: "Copy as cURL", shortcut: shortcutLabel("Shift+C"), onSelect: () => void copyAsCurl() },
          { label: "Manage cookies…", onSelect: () => showDialog("cookies") },
        ]}
      />
      <Menu
        label="View"
        items={[{ label: "Toggle Sidebar", shortcut: shortcutLabel("B"), onSelect: toggleSidebar }]}
      />
    </div>
  );
}
