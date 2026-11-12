import { useId, useState } from "react";
import type { ScratchItem, TreeNode } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { useCollectionsStore } from "../collections/collectionsStore";
import { useScratchStore } from "./scratchStore";

export interface MoveToCollectionDialogProps {
  readonly item: ScratchItem;
  readonly onClose: () => void;
}

interface FolderOption {
  readonly path: string;
  readonly label: string;
}

const ROOT_LABEL = "(root)";
const INDENT = "  ";

function folderOptions(nodes: readonly TreeNode[], depth: number): readonly FolderOption[] {
  return nodes
    .filter((node) => node.kind === "folder")
    .flatMap((node) => [
      { path: node.path, label: `${INDENT.repeat(depth)}${node.name}` },
      ...folderOptions(node.children, depth + 1),
    ]);
}

/** Moves a scratch request into a chosen collection and folder. Mount only while shown. */
export function MoveToCollectionDialog({ item, onClose }: MoveToCollectionDialogProps) {
  const id = useId();
  const trees = useCollectionsStore((s) => s.trees).filter((t) => t.collection.available);
  const [chosenId, setChosenId] = useState<string | null>(null);
  const [folder, setFolder] = useState("");
  const [busy, setBusy] = useState(false);

  const tree = trees.find((t) => t.collection.id === chosenId) ?? trees[0];
  const options: readonly FolderOption[] = tree
    ? [{ path: "", label: ROOT_LABEL }, ...folderOptions(tree.children, 0)]
    : [];

  const submit = async () => {
    if (!tree) return;
    setBusy(true);
    const done = await useScratchStore.getState().moveToCollection(item.id, tree.collection.id, folder);
    setBusy(false);
    if (done) onClose();
  };

  return (
    <Dialog
      open
      title="Move to collection"
      onClose={onClose}
      actions={<Button label="Move" disabled={!tree || busy} onClick={() => void submit()} />}
    >
      {tree ? (
        <>
          <div className="field-row">
            <label htmlFor={`${id}-collection`}>Collection</label>
            <select
              id={`${id}-collection`}
              value={tree.collection.id}
              onChange={(event) => {
                setChosenId(event.target.value);
                setFolder("");
              }}
            >
              {trees.map((t) => (
                <option key={t.collection.id} value={t.collection.id}>
                  {t.collection.name}
                </option>
              ))}
            </select>
          </div>
          <div className="field-row">
            <label htmlFor={`${id}-folder`}>Folder</label>
            <select id={`${id}-folder`} value={folder} onChange={(event) => setFolder(event.target.value)}>
              {options.map((option) => (
                <option key={option.path} value={option.path}>
                  {option.label}
                </option>
              ))}
            </select>
          </div>
        </>
      ) : (
        <p className="dialog-message">Open or create a collection first.</p>
      )}
    </Dialog>
  );
}
