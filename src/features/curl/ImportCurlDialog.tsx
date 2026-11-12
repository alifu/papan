import { useState } from "react";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { reportError } from "../../ui/toastStore";
import { useTabsStore } from "../../shell/tabsStore";
import { writeRequest } from "../collections/api";
import { sameLocation } from "../locations";
import { useRequestStore } from "../request/requestStore";
import { useScratchStore } from "../scratch/scratchStore";
import * as api from "./api";

export interface ImportCurlDialogProps {
  readonly onClose: () => void;
}

/** File › Import cURL…: parses the pasted command into a new scratch request. */
export function ImportCurlDialog({ onClose }: ImportCurlDialogProps) {
  const [text, setText] = useState("");
  const [warnings, setWarnings] = useState<readonly string[]>([]);
  const [busy, setBusy] = useState(false);

  const create = async () => {
    setBusy(true);
    try {
      const parsed = await api.importCurl(text);
      const item = await useScratchStore.getState().create();
      if (!item) return;
      const location = { scope: "scratch" as const, id: item.id };
      await writeRequest(location, parsed.content);
      // The tab opened on the empty request; reload it with the imported content.
      const tab = useTabsStore.getState().tabs.find((t) => t.location && sameLocation(t.location, location));
      if (tab) await useRequestStore.getState().load(tab.id, location);
      if (parsed.warnings.length === 0) return onClose();
      setWarnings(parsed.warnings);
    } catch (err) {
      reportError(err);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      title="Import cURL"
      onClose={onClose}
      cancelLabel={warnings.length > 0 ? "Close" : "Cancel"}
      actions={warnings.length > 0 ? null : <Button label="Create request" disabled={busy || text.trim() === ""} onClick={() => void create()} />}
    >
      <label>
        cURL command
        <textarea rows={8} spellCheck={false} value={text} onChange={(e) => setText(e.target.value)} />
      </label>
      {warnings.length > 0 ? (
        <>
          <p>Request created. Some options were ignored:</p>
          <ul aria-label="Import warnings">
            {warnings.map((w, i) => (
              <li key={i}>{w}</li>
            ))}
          </ul>
        </>
      ) : null}
    </Dialog>
  );
}
