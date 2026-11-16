import { CollectionsPanel } from "../features/collections/CollectionsPanel";
import { HistoryPanel } from "../features/history/HistoryPanel";
import { JourneysPanel } from "../features/journeys/JourneysPanel";
import { MocksPanel } from "../features/mocks/MocksPanel";
import { ScratchPanel } from "../features/scratch/ScratchPanel";
import { WebhooksPanel } from "../features/webhooks/WebhooksPanel";
import { Button } from "../ui/Button";
import { Icon } from "../ui/icons";
import { NAV_SECTIONS, useLayoutStore, type NavSection } from "./layoutStore";
import { NAV_SECTION_META } from "./navSections";

function renderSection(section: NavSection, emptyText: string) {
  if (section === "collections") return <CollectionsPanel />;
  if (section === "scratch") return <ScratchPanel />;
  if (section === "webhooks") return <WebhooksPanel />;
  if (section === "mocks") return <MocksPanel />;
  if (section === "journeys") return <JourneysPanel />;
  if (section === "history") return <HistoryPanel />;
  return <p className="empty-text">{emptyText}</p>;
}

export function Sidebar() {
  const collapsed = useLayoutStore((s) => s.sidebarCollapsed);
  const activeSection = useLayoutStore((s) => s.activeSection);
  const setActiveSection = useLayoutStore((s) => s.setActiveSection);
  const toggleSidebar = useLayoutStore((s) => s.toggleSidebar);

  if (collapsed) {
    return (
      <nav aria-label="Navigation" className="sidebar sidebar-rail">
        <Button
          icon="chevron-right"
          aria-label="Expand sidebar"
          onClick={toggleSidebar}
        />
      </nav>
    );
  }

  return (
    <nav aria-label="Navigation" className="sidebar">
      {NAV_SECTIONS.map((section, index) => {
        const meta = NAV_SECTION_META[section];
        const expanded = section === activeSection;
        return (
          <section
            key={section}
            className={expanded ? "accordion accordion-open" : "accordion"}
          >
            <div className="accordion-header">
              <h2>
                <button
                  type="button"
                  id={`nav-${section}-header`}
                  aria-expanded={expanded}
                  aria-controls={`nav-${section}-panel`}
                  onClick={() => setActiveSection(section)}
                >
                  <Icon name={meta.icon} />
                  <span>{meta.label}</span>
                </button>
              </h2>
              {index === 0 ? (
                <Button
                  icon="chevron-left"
                  aria-label="Collapse sidebar"
                  onClick={toggleSidebar}
                />
              ) : null}
            </div>
            {expanded ? (
              <div
                role="region"
                id={`nav-${section}-panel`}
                aria-labelledby={`nav-${section}-header`}
                className="accordion-panel"
              >
                {renderSection(section, meta.emptyText)}
              </div>
            ) : null}
          </section>
        );
      })}
    </nav>
  );
}
