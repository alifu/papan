import { Button } from "../ui/Button";
import { shortcutLabel } from "../ui/platform";
import { TAB_PANEL_ID, tabButtonId } from "./TabStrip";
import { useTabsStore, type Tab } from "./tabsStore";

function Welcome() {
  return (
    <div className="welcome">
      <h1>Welcome to Papan</h1>
      <p>Build, send and inspect API requests. Open a tab to get started.</p>
      <ul>
        <li>{shortcutLabel("N")} New tab</li>
        <li>{shortcutLabel("W")} Close tab</li>
        <li>{shortcutLabel("B")} Toggle sidebar</li>
      </ul>
    </div>
  );
}

function renderTab(tab: Tab) {
  return tab.kind === "welcome" ? <Welcome /> : <p className="empty-text">Nothing to show yet.</p>;
}

export function TabPanel() {
  const tabs = useTabsStore((s) => s.tabs);
  const activeTabId = useTabsStore((s) => s.activeTabId);
  const newTab = useTabsStore((s) => s.newTab);
  const active = tabs.find((t) => t.id === activeTabId);

  if (!active) {
    return (
      <div role="tabpanel" id={TAB_PANEL_ID} aria-label="No open tabs" className="tabpanel">
        <p className="empty-text">No open tabs.</p>
        <Button icon="plus" label="New Tab" onClick={newTab} />
      </div>
    );
  }

  return (
    <div role="tabpanel" id={TAB_PANEL_ID} aria-labelledby={tabButtonId(active.id)} className="tabpanel">
      {renderTab(active)}
    </div>
  );
}
