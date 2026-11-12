import type { KeyboardEvent } from "react";
import { Button } from "../ui/Button";
import { useTabsStore } from "./tabsStore";

export const TAB_PANEL_ID = "tab-panel";
export const tabButtonId = (id: string): string => `tab-button-${id}`;

export function TabStrip() {
  const tabs = useTabsStore((s) => s.tabs);
  const activeTabId = useTabsStore((s) => s.activeTabId);
  const activateTab = useTabsStore((s) => s.activateTab);
  const closeTab = useTabsStore((s) => s.closeTab);
  const newTab = useTabsStore((s) => s.newTab);

  const handleKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const offset = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    if (offset === 0) return;
    event.preventDefault();
    const target = tabs[(index + offset + tabs.length) % tabs.length];
    activateTab(target.id);
    document.getElementById(tabButtonId(target.id))?.focus();
  };

  return (
    <div className="tabstrip">
      <div role="tablist" aria-label="Open tabs" className="tablist">
        {tabs.map((tab, index) => {
          const selected = tab.id === activeTabId;
          return (
            <div key={tab.id} className={selected ? "tab tab-selected" : "tab"}>
              <button
                type="button"
                role="tab"
                id={tabButtonId(tab.id)}
                aria-selected={selected}
                aria-controls={TAB_PANEL_ID}
                tabIndex={selected ? 0 : -1}
                onClick={() => activateTab(tab.id)}
                onKeyDown={(event) => handleKeyDown(event, index)}
              >
                {tab.title}
              </button>
              <Button
                icon="close"
                className="tab-close"
                aria-label={`Close ${tab.title}`}
                onClick={() => closeTab(tab.id)}
              />
            </div>
          );
        })}
      </div>
      <Button icon="plus" aria-label="New tab" onClick={() => void newTab()} />
    </div>
  );
}
