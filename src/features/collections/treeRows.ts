import type { CollectionTree, TreeNode } from "../../ipc/bindings";
import { expandKey } from "./collectionsStore";

export type RowKind = "collection" | "folder" | "request" | "invalid";

/** One visible line of the tree. The collection itself is the level-1 row. */
export interface TreeRowModel {
  readonly key: string;
  readonly collectionId: string;
  readonly path: string;
  readonly parentPath: string;
  readonly name: string;
  readonly kind: RowKind;
  readonly method: string | null;
  readonly level: number;
  /** Position among siblings, 0-based. */
  readonly index: number;
  readonly siblingCount: number;
  readonly childCount: number;
  readonly expanded: boolean | undefined;
  readonly available: boolean;
}

function nodeRows(
  collectionId: string,
  nodes: readonly TreeNode[],
  parentPath: string,
  level: number,
  expanded: ReadonlySet<string>,
): readonly TreeRowModel[] {
  return nodes.flatMap((node, index) => {
    const isFolder = node.kind === "folder";
    const isOpen = isFolder && expanded.has(expandKey(collectionId, node.path));
    const row: TreeRowModel = {
      key: expandKey(collectionId, node.path),
      collectionId,
      path: node.path,
      parentPath,
      name: node.name,
      kind: node.kind,
      method: node.method,
      level,
      index,
      siblingCount: nodes.length,
      childCount: node.children.length,
      expanded: isFolder ? isOpen : undefined,
      available: true,
    };
    const below = isOpen ? nodeRows(collectionId, node.children, node.path, level + 1, expanded) : [];
    return [row, ...below];
  });
}

/** Flattens the trees into the rows that are currently visible. */
export function visibleRows(
  trees: readonly CollectionTree[],
  expanded: ReadonlySet<string>,
): readonly TreeRowModel[] {
  return trees.flatMap((tree) => {
    const id = tree.collection.id;
    const isOpen = tree.collection.available && expanded.has(expandKey(id, ""));
    const root: TreeRowModel = {
      key: expandKey(id, ""),
      collectionId: id,
      path: "",
      parentPath: "",
      name: tree.collection.name,
      kind: "collection",
      method: null,
      level: 1,
      index: 0,
      siblingCount: 1,
      childCount: tree.children.length,
      expanded: tree.collection.available ? isOpen : undefined,
      available: tree.collection.available,
    };
    return [root, ...(isOpen ? nodeRows(id, tree.children, "", 2, expanded) : [])];
  });
}
