import { useEffect, useState } from "react";
import type { ScriptTarget, Scripts } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { reportError } from "../../ui/toastStore";
import { ScriptsEditor, type ScriptPhase } from "../request/ScriptsEditor";
import { readScripts, writeScripts } from "./api";

export interface ScriptsDialogProps {
  readonly title: string;
  readonly target: ScriptTarget;
  readonly onClose: () => void;
}

/** Edits the scripts of a folder or collection. Mount it only while it is shown. */
export function ScriptsDialog({ title, target, onClose }: ScriptsDialogProps) {
  const [scripts, setScripts] = useState<Scripts | null>(null);
  const [phase, setPhase] = useState<ScriptPhase>("pre_request");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    readScripts(target).then(
      (loaded) => live && setScripts(loaded),
      (err: unknown) => {
        reportError(err);
        if (live) onClose();
      },
    );
    return () => {
      live = false;
    };
    // The dialog is keyed to one target for its whole lifetime.
  }, []);

  const save = async () => {
    if (!scripts) return;
    setBusy(true);
    try {
      await writeScripts(target, scripts);
      onClose();
    } catch (err) {
      reportError(err);
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      title={title}
      onClose={onClose}
      actions={<Button label="Save" disabled={scripts === null || busy} onClick={() => void save()} />}
    >
      {scripts ? <ScriptsEditor scripts={scripts} phase={phase} onPhase={setPhase} onChange={setScripts} /> : <p>Loading...</p>}
    </Dialog>
  );
}
