import type { DragEvent } from "react";
import { Icon } from "../../ui/icons";
import { Menu } from "../../ui/Menu";
import { menuItemsFor, type RowActions } from "./rowMenu";
import type { TreeRowModel } from "./treeRows";

export interface TreeRowProps {
  readonly row: TreeRowModel;
  readonly focused: boolean;
  readonly actions: RowActions;
  readonly onActivate: (row: TreeRowModel) => void;
  readonly onFocusRow: (row: TreeRowModel) => void;
  readonly onDragStart: (row: TreeRowModel, event: DragEvent) => void;
  readonly onDragOver: (row: TreeRowModel, event: DragEvent) => void;
  readonly onDrop: (row: TreeRowModel, event: DragEvent) => void;
}

function rowIcon(row: TreeRowModel) {
  if (row.kind === "request") return "request-http";
  return row.expanded ? "folder-open" : "folder";
}

export function TreeRow({
  row,
  focused,
  actions,
  onActivate,
  onFocusRow,
  onDragStart,
  onDragOver,
  onDrop,
}: TreeRowProps) {
  const draggable = row.kind === "folder" || row.kind === "request";
  const classes = ["tree-row", `tree-${row.kind}`, !row.available ? "tree-missing" : ""]
    .filter(Boolean)
    .join(" ");
  const label = row.available ? row.name : `${row.name} (missing)`;
  return (
    <div
      role="treeitem"
      aria-level={row.level}
      aria-expanded={row.expanded}
      aria-label={label}
      data-row-key={row.key}
      tabIndex={focused ? 0 : -1}
      className={classes}
      style={{ paddingLeft: 4 + (row.level - 1) * 14 }}
      draggable={draggable}
      onClick={() => onActivate(row)}
      onFocus={() => onFocusRow(row)}
      onDragStart={(event) => onDragStart(row, event)}
      onDragOver={(event) => onDragOver(row, event)}
      onDrop={(event) => onDrop(row, event)}
    >
      <Icon name={rowIcon(row)} />
      <span className="tree-label">{label}</span>
      {row.method ? <span className="tree-method">{row.method}</span> : null}
      {/* The menu must not trigger row activation or tree key handling. */}
      <span
        className="tree-menu-slot"
        role="none"
        onClick={(event) => event.stopPropagation()}
        onKeyDown={(event) => event.stopPropagation()}
      >
        <Menu
          label={`Actions for ${row.name}`}
          icon="chevron-down"
          className="tree-menu"
          items={menuItemsFor(row, actions)}
        />
      </span>
    </div>
  );
}
