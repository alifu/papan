import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { Icon, type IconName } from "./icons";

export interface MenuItem {
  readonly label: string;
  readonly shortcut?: string;
  readonly onSelect: () => void;
  readonly disabled?: boolean;
}

export interface MenuProps {
  readonly label: string;
  readonly items: readonly MenuItem[];
  /** Shows an icon-only trigger; `label` stays its accessible name. */
  readonly icon?: IconName;
  readonly className?: string;
}

/** Menu-bar dropdown with roving focus. */
export function Menu({ label, items, icon, className }: MenuProps) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);

  useEffect(() => {
    if (open) itemRefs.current[0]?.focus();
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: Event) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [open]);

  const focusAt = (index: number) => {
    const count = items.length;
    itemRefs.current[(index + count) % count]?.focus();
  };

  const select = (item: MenuItem) => {
    if (item.disabled) return;
    setOpen(false);
    triggerRef.current?.focus();
    item.onSelect();
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLElement>, index: number) => {
    const handlers: Record<string, () => void> = {
      ArrowDown: () => focusAt(index + 1),
      ArrowUp: () => focusAt(index - 1),
      Enter: () => select(items[index]),
      Escape: () => {
        setOpen(false);
        triggerRef.current?.focus();
      },
      Tab: () => setOpen(false),
    };
    const handler = handlers[event.key];
    if (!handler) return;
    if (event.key !== "Tab") event.preventDefault();
    handler();
  };

  return (
    <div className={className ? `menu ${className}` : "menu"} role="none" ref={rootRef}>
      <button
        ref={triggerRef}
        type="button"
        role="menuitem"
        className="menu-trigger"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={icon ? label : undefined}
        onClick={() => setOpen((current) => !current)}
      >
        {icon ? <Icon name={icon} /> : label}
      </button>
      {open ? (
        <div role="menu" aria-label={label} className="menu-popup">
          {items.map((item, index) => (
            <button
              key={item.label}
              ref={(el) => {
                itemRefs.current[index] = el;
              }}
              type="button"
              role="menuitem"
              className="menu-item"
              disabled={item.disabled}
              onKeyDown={(event) => handleKeyDown(event, index)}
              onClick={() => select(item)}
            >
              <span>{item.label}</span>
              {item.shortcut ? <span className="menu-shortcut">{item.shortcut}</span> : null}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}
