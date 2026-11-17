import { useState } from "react";
import type { RequestKind, ScriptTarget } from "../../ipc/bindings";
import { useDialogsStore } from "../../shell/dialogsStore";
import { useTabsStore } from "../../shell/tabsStore";
import { Button } from "../../ui/Button";
import { reportError } from "../../ui/toastStore";
import { collectionLocation } from "../locations";
import * as api from "./api";
import { ConfirmDialog } from "./ConfirmDialog";
import { useCollectionsStore } from "./collectionsStore";
import { NameDialog } from "./NameDialog";
import type { RowActions } from "./rowMenu";
import { ScriptsDialog } from "./ScriptsDialog";
import { TreeView } from "./TreeView";
import type { TreeRowModel } from "./treeRows";
import "./collections.css";

type DialogState =
  | { readonly kind: "newCollection" }
  | {
      readonly kind: "newFolder" | "newRequest" | "rename";
      readonly row: TreeRowModel;
    }
  | { readonly kind: "delete"; readonly row: TreeRowModel }
  | { readonly kind: "scripts"; readonly row: TreeRowModel }
  | { readonly kind: "trust"; readonly row: TreeRowModel };

const EMPTY_TEXT = "No collections yet.";

function deleteMessage(row: TreeRowModel): string {
  return row.kind === "folder"
    ? `Delete the folder "${row.name}" and everything in it? This cannot be undone.`
    : `Delete "${row.name}"? This cannot be undone.`;
}

export function CollectionsPanel() {
  const trees = useCollectionsStore((s) => s.trees);
  const store = useCollectionsStore;
  const [dialog, setDialog] = useState<DialogState | null>(null);
  const closeDialog = () => setDialog(null);

  const openRequestTab = (collectionId: string, path: string, title: string) =>
    useTabsStore
      .getState()
      .openRequest(collectionLocation(collectionId, path), title);

  const submitName = async (
    state: DialogState,
    name: string,
    kind: RequestKind,
  ): Promise<boolean> => {
    const s = store.getState();
    if (state.kind === "newCollection") return (await s.create(name)) !== null;
    if (state.kind === "delete" || state.kind === "scripts") return false;
    const { collectionId, path } = state.row;
    if (state.kind === "newFolder")
      return (await s.createFolder(collectionId, path, name)) !== null;
    if (state.kind === "newRequest") {
      const change = await s.createRequest(collectionId, path, name, kind);
      if (change?.path) openRequestTab(collectionId, change.path, name);
      return change !== null;
    }
    const change = await s.rename(collectionId, path, name);
    if (change?.path && state.row.kind !== "invalid") {
      useTabsStore
        .getState()
        .retarget(
          collectionLocation(collectionId, path),
          collectionLocation(collectionId, change.path),
          name,
        );
    }
    return change !== null;
  };

  const confirmTrust = async (row: TreeRowModel) => {
    closeDialog();
    try {
      await api.setScriptsTrusted(row.collectionId, true);
      await store.getState().refresh(row.collectionId);
    } catch (err) {
      reportError(err);
    }
  };

  const confirmDelete = async (row: TreeRowModel) => {
    closeDialog();
    const change = await store.getState().remove(row.collectionId, row.path);
    if (change) {
      useTabsStore
        .getState()
        .closeByLocation(collectionLocation(row.collectionId, row.path));
    }
  };

  const move = async (row: TreeRowModel, folder: string, index: number) => {
    const change = await store
      .getState()
      .move(row.collectionId, row.path, folder, index);
    if (change?.path) {
      useTabsStore
        .getState()
        .retarget(
          collectionLocation(row.collectionId, row.path),
          collectionLocation(row.collectionId, change.path),
        );
    }
  };

  const actions: RowActions = {
    newRequest: (row) => setDialog({ kind: "newRequest", row }),
    newFolder: (row) => setDialog({ kind: "newFolder", row }),
    scripts: (row) => setDialog({ kind: "scripts", row }),
    trustScripts: (row) => setDialog({ kind: "trust", row }),
    git: (row) => useDialogsStore.getState().show("git", row.collectionId),
    exportPostman: (row) =>
      useDialogsStore.getState().show("export-postman", row.collectionId),
    rename: (row) => setDialog({ kind: "rename", row }),
    remove: (row) => setDialog({ kind: "delete", row }),
    closeCollection: (row) => {
      void store
        .getState()
        .close(row.collectionId)
        .then((closed) => {
          if (closed) {
            useTabsStore
              .getState()
              .closeByLocation(collectionLocation(row.collectionId, ""));
          }
        });
    },
    moveBy: (row, offset) => void move(row, row.parentPath, row.index + offset),
  };

  return (
    <div className="collections-panel">
      <div className="panel-actions">
        <Button
          icon="plus"
          label="New Collection"
          onClick={() => setDialog({ kind: "newCollection" })}
        />
        <Button
          icon="folder-open"
          label="Open Collection"
          onClick={() => void store.getState().open()}
        />
      </div>
      {trees.length === 0 ? (
        <p className="empty-text">{EMPTY_TEXT}</p>
      ) : (
        <TreeView
          trees={trees}
          actions={actions}
          onOpenRequest={(row) =>
            openRequestTab(row.collectionId, row.path, row.name)
          }
          onMove={(row, folder, index) => void move(row, folder, index)}
        />
      )}
      {dialog &&
      dialog.kind !== "delete" &&
      dialog.kind !== "scripts" &&
      dialog.kind !== "trust" ? (
        <NameDialog
          title={dialogTitle(dialog)}
          initialName={dialog.kind === "rename" ? dialog.row.name : ""}
          submitLabel={dialog.kind === "rename" ? "Rename" : "Create"}
          withKind={dialog.kind === "newRequest"}
          onSubmit={(name, kind) => submitName(dialog, name, kind)}
          onClose={closeDialog}
        />
      ) : null}
      {dialog?.kind === "scripts" ? (
        <ScriptsDialog
          title={`Scripts: ${dialog.row.name}`}
          target={scriptTarget(dialog.row)}
          onClose={closeDialog}
        />
      ) : null}
      {dialog?.kind === "trust" ? (
        <ConfirmDialog
          title="Trust scripts"
          message={`“${dialog.row.name}” came from a Git repository. Its scripts can read your secrets and send requests. Only trust them if you trust whoever wrote them.`}
          confirmLabel="Trust"
          onConfirm={() => void confirmTrust(dialog.row)}
          onClose={closeDialog}
        />
      ) : null}
      {dialog?.kind === "delete" ? (
        <ConfirmDialog
          title="Delete"
          message={deleteMessage(dialog.row)}
          onConfirm={() => void confirmDelete(dialog.row)}
          onClose={closeDialog}
        />
      ) : null}
    </div>
  );
}

const scriptTarget = (row: TreeRowModel): ScriptTarget =>
  row.kind === "collection"
    ? { scope: "collection", collection_id: row.collectionId }
    : { scope: "folder", collection_id: row.collectionId, path: row.path };

function dialogTitle(
  dialog: Exclude<DialogState, { kind: "delete" | "scripts" | "trust" }>,
): string {
  switch (dialog.kind) {
    case "newCollection":
      return "New Collection";
    case "newFolder":
      return "New Folder";
    case "newRequest":
      return "New Request";
    case "rename":
      return "Rename";
  }
}
