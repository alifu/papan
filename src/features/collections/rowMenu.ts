import type { MenuItem } from "../../ui/Menu";
import type { TreeRowModel } from "./treeRows";

export interface RowActions {
  readonly newRequest: (row: TreeRowModel) => void;
  readonly newFolder: (row: TreeRowModel) => void;
  readonly scripts: (row: TreeRowModel) => void;
  readonly rename: (row: TreeRowModel) => void;
  readonly remove: (row: TreeRowModel) => void;
  readonly exportPostman: (row: TreeRowModel) => void;
  readonly git: (row: TreeRowModel) => void;
  readonly trustScripts: (row: TreeRowModel) => void;
  readonly closeCollection: (row: TreeRowModel) => void;
  readonly moveBy: (row: TreeRowModel, offset: -1 | 1) => void;
}

/** Menu entries for a row, by node kind. */
export function menuItemsFor(
  row: TreeRowModel,
  actions: RowActions,
): readonly MenuItem[] {
  const item = (
    label: string,
    run: (r: TreeRowModel) => void,
    disabled = false,
  ): MenuItem => ({
    label,
    disabled,
    onSelect: () => run(row),
  });
  const mover = [
    item("Move Up", (r) => actions.moveBy(r, -1), row.index === 0),
    item(
      "Move Down",
      (r) => actions.moveBy(r, 1),
      row.index >= row.siblingCount - 1,
    ),
  ];
  switch (row.kind) {
    case "collection":
      return row.available
        ? [
            item("New Request", actions.newRequest),
            item("New Folder", actions.newFolder),
            item("Scripts…", actions.scripts),
            ...(row.scriptsTrusted
              ? []
              : [item("Trust scripts…", actions.trustScripts)]),
            item("Export to Postman…", actions.exportPostman),
            item("Git…", actions.git),
            item("Close", actions.closeCollection),
          ]
        : [item("Close", actions.closeCollection)];
    case "folder":
      return [
        item("New Request", actions.newRequest),
        item("New Folder", actions.newFolder),
        item("Scripts…", actions.scripts),
        item("Rename", actions.rename),
        ...mover,
        item("Delete", actions.remove),
      ];
    case "request":
      return [
        item("Rename", actions.rename),
        ...mover,
        item("Delete", actions.remove),
      ];
    case "invalid":
      return [item("Delete", actions.remove)];
  }
}
