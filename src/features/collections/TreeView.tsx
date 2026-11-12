import { useRef, useState, type DragEvent, type KeyboardEvent } from "react";
import type { CollectionTree } from "../../ipc/bindings";
import { isWithin } from "../locations";
import { useCollectionsStore } from "./collectionsStore";
import type { RowActions } from "./rowMenu";
import { TreeRow } from "./TreeRow";
import { visibleRows, type TreeRowModel } from "./treeRows";

export interface TreeViewProps {
  readonly trees: readonly CollectionTree[];
  readonly actions: RowActions;
  readonly onOpenRequest: (row: TreeRowModel) => void;
  readonly onMove: (row: TreeRowModel, folder: string, index: number) => void;
}

function isDropTarget(dragged: TreeRowModel | null, target: TreeRowModel): dragged is TreeRowModel {
  if (!dragged || dragged.collectionId !== target.collectionId) return false;
  if (target.kind !== "request" && target.kind !== "folder") return false;
  if (dragged.key === target.key) return false;
  return !(dragged.kind === "folder" && isWithin(dragged.path, target.path));
}

export function TreeView({ trees, actions, onOpenRequest, onMove }: TreeViewProps) {
  const expanded = useCollectionsStore((s) => s.expanded);
  const toggle = useCollectionsStore((s) => s.toggle);
  const expand = useCollectionsStore((s) => s.expand);
  const rows = visibleRows(trees, expanded);
  const [focusKey, setFocusKey] = useState<string | null>(null);
  const dragged = useRef<TreeRowModel | null>(null);
  const activeKey = rows.some((r) => r.key === focusKey) ? focusKey : (rows[0]?.key ?? null);

  const focusRow = (row: TreeRowModel | undefined) => {
    if (!row) return;
    setFocusKey(row.key);
    document.querySelector<HTMLElement>(`[data-row-key="${CSS.escape(row.key)}"]`)?.focus();
  };

  const activate = (row: TreeRowModel) => {
    setFocusKey(row.key);
    if (row.kind === "request") onOpenRequest(row);
    else if (row.expanded !== undefined) toggle(row.key);
  };

  const parentOf = (row: TreeRowModel) =>
    rows.find(
      (r) =>
        r.collectionId === row.collectionId &&
        r.level === row.level - 1 &&
        (row.level === 2 ? r.kind === "collection" : r.path === row.parentPath),
    );

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const index = rows.findIndex((r) => r.key === activeKey);
    const row = rows[index];
    if (!row) return;
    const handlers: Record<string, () => void> = {
      ArrowDown: () => focusRow(rows[index + 1]),
      ArrowUp: () => focusRow(rows[index - 1]),
      Home: () => focusRow(rows[0]),
      End: () => focusRow(rows[rows.length - 1]),
      ArrowRight: () => {
        if (row.expanded === false) expand(row.key);
        else if (row.expanded && rows[index + 1]?.level === row.level + 1) focusRow(rows[index + 1]);
      },
      ArrowLeft: () => {
        if (row.expanded) toggle(row.key);
        else focusRow(parentOf(row));
      },
      Enter: () => activate(row),
    };
    const handler = handlers[event.key];
    if (!handler) return;
    event.preventDefault();
    handler();
  };

  const handleDragStart = (row: TreeRowModel, event: DragEvent) => {
    if (row.kind !== "request" && row.kind !== "folder") {
      event.preventDefault();
      return;
    }
    dragged.current = row;
    event.dataTransfer?.setData("text/plain", row.path);
  };

  const handleDragOver = (row: TreeRowModel, event: DragEvent) => {
    if (isDropTarget(dragged.current, row)) event.preventDefault();
  };

  const handleDrop = (target: TreeRowModel, event: DragEvent) => {
    const source = dragged.current;
    dragged.current = null;
    if (!isDropTarget(source, target)) return;
    event.preventDefault();
    if (target.kind === "folder") {
      onMove(source, target.path, target.childCount);
      return;
    }
    const sameFolder = source.parentPath === target.parentPath;
    const index = sameFolder && source.index < target.index ? target.index - 1 : target.index;
    onMove(source, target.parentPath, index);
  };

  return (
    <div
      role="tree"
      aria-label="Collections"
      className="tree"
      onKeyDown={handleKeyDown}
      onDragEnd={() => {
        dragged.current = null;
      }}
    >
      {rows.map((row) => (
        <TreeRow
          key={row.key}
          row={row}
          focused={row.key === activeKey}
          actions={actions}
          onActivate={activate}
          onFocusRow={(r) => setFocusKey(r.key)}
          onDragStart={handleDragStart}
          onDragOver={handleDragOver}
          onDrop={handleDrop}
        />
      ))}
    </div>
  );
}
