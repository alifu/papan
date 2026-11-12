import { useId, useRef, type KeyboardEvent, type ReactNode } from "react";

export interface TabItem {
  readonly id: string;
  readonly label: string;
  readonly count?: number;
}

export interface TabsProps {
  readonly tabs: readonly TabItem[];
  readonly selectedId: string;
  readonly onSelect: (id: string) => void;
  readonly label: string;
  /** Content of the selected tab. */
  readonly children?: ReactNode;
}

/** Sub-tabs with roving tabindex; arrow keys, Home and End move and select. */
export function Tabs({ tabs, selectedId, onSelect, label, children }: TabsProps) {
  const uid = useId();
  const buttons = useRef(new Map<string, HTMLButtonElement>());

  const move = (id: string) => {
    onSelect(id);
    buttons.current.get(id)?.focus();
  };

  const onKeyDown = (event: KeyboardEvent, index: number) => {
    const last = tabs.length - 1;
    const target: Record<string, number> = {
      ArrowRight: index === last ? 0 : index + 1,
      ArrowLeft: index === 0 ? last : index - 1,
      Home: 0,
      End: last,
    };
    const next = tabs[target[event.key] ?? -1];
    if (!next) return;
    event.preventDefault();
    move(next.id);
  };

  return (
    <div className="tabs">
      <div role="tablist" aria-label={label} className="tabs-list">
        {tabs.map((tab, index) => {
          const selected = tab.id === selectedId;
          return (
            <button
              key={tab.id}
              ref={(el) => {
                if (el) buttons.current.set(tab.id, el);
                else buttons.current.delete(tab.id);
              }}
              type="button"
              role="tab"
              id={`${uid}-tab-${tab.id}`}
              aria-selected={selected}
              aria-controls={`${uid}-panel`}
              tabIndex={selected ? 0 : -1}
              className="tabs-tab"
              onClick={() => onSelect(tab.id)}
              onKeyDown={(event) => onKeyDown(event, index)}
            >
              {tab.label}
              {tab.count !== undefined && tab.count > 0 ? ` (${tab.count})` : ""}
            </button>
          );
        })}
      </div>
      <div
        role="tabpanel"
        id={`${uid}-panel`}
        aria-labelledby={`${uid}-tab-${selectedId}`}
        className="tabs-panel"
      >
        {children}
      </div>
    </div>
  );
}
