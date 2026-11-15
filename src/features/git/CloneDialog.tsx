import { useState } from "react";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { reportError } from "../../ui/toastStore";
import {
  expandKey,
  useCollectionsStore,
} from "../collections/collectionsStore";
import * as api from "./api";

export interface CloneDialogProps {
  readonly onClose: () => void;
}

/** File > Clone Repository…: asks for a URL, then Rust asks for the parent folder. */
export function CloneDialog({ onClose }: CloneDialogProps) {
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);

  const run = async () => {
    setBusy(true);
    try {
      const tree = await api.clone(url.trim());
      if (!tree) return; // folder picker cancelled
      await useCollectionsStore.getState().load();
      useCollectionsStore.getState().expand(expandKey(tree.collection.id, ""));
      onClose();
    } catch (err) {
      reportError(err);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      title="Clone Repository"
      onClose={onClose}
      actions={
        <Button
          label="Clone"
          disabled={busy || url.trim() === ""}
          onClick={() => void run()}
        />
      }
    >
      <label>
        Repository URL
        <input
          aria-label="Repository URL"
          value={url}
          placeholder="https://github.com/team/api-collection.git"
          onChange={(e) => setUrl(e.target.value)}
        />
      </label>
      <p>
        Git uses your existing SSH keys and credential helpers. The repository
        must be a Papan collection.
      </p>
    </Dialog>
  );
}
