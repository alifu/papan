import { useEffect, useRef, useState } from "react";
import type { PostmanImportResult, PostmanPreview } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { reportError } from "../../ui/toastStore";
import { expandKey, useCollectionsStore } from "../collections/collectionsStore";
import { useEnvStore } from "../environments/envStore";
import * as api from "./api";
import { ImportReport } from "./ImportReport";
import {
  defaultChoices,
  hasGlobals,
  importBlocker,
  needsCollection,
  secretCandidates,
  toggleName,
  toImportOptions,
  type ImportChoices,
} from "./postmanModel";

export interface ImportPostmanDialogProps {
  readonly onClose: () => void;
}

const SCRIPTS_NOTE =
  "Scripts are imported switched off. Read them, then enable them per script: an enabled script runs on Send.";

/** File > Import Postman...: pick files, review the preview and report, then commit. */
export function ImportPostmanDialog({ onClose }: ImportPostmanDialogProps) {
  const trees = useCollectionsStore((s) => s.trees);
  const [preview, setPreview] = useState<PostmanPreview | null>(null);
  const [choices, setChoices] = useState<ImportChoices | null>(null);
  const [result, setResult] = useState<PostmanImportResult | null>(null);
  const [busy, setBusy] = useState(false);
  // Set once the dialog is gone, so a late pick or commit cannot touch state or leave a stage behind.
  const closed = useRef(false);
  const token = useRef<string | null>(null);

  useEffect(() => {
    closed.current = false;
    return () => {
      closed.current = true;
    };
  }, []);

  const discard = () => {
    const pending = token.current;
    token.current = null;
    if (pending) api.importDiscard(pending).catch(reportError);
  };

  const close = () => {
    discard();
    onClose();
  };

  const pick = async () => {
    setBusy(true);
    try {
      const next = await api.importPick();
      if (!next) return; // user cancelled the file dialog
      if (closed.current) {
        api.importDiscard(next.token).catch(reportError);
        return;
      }
      token.current = next.token;
      setPreview(next);
      setChoices(defaultChoices(next, trees.map((t) => t.collection.id)));
    } catch (err) {
      reportError(err);
    } finally {
      if (!closed.current) setBusy(false);
    }
  };

  const commit = async () => {
    if (!preview || !choices) return;
    setBusy(true);
    try {
      const done = await api.importCommit(preview.token, toImportOptions(choices));
      if (!done) return; // user cancelled the folder dialog: the preview stays
      token.current = null;
      try {
        await refreshStores(done);
      } catch (err) {
        reportError(err); // the import itself worked: still show its report
      }
      if (!closed.current) setResult(done);
    } catch (err) {
      reportError(err);
    } finally {
      if (!closed.current) setBusy(false);
    }
  };

  if (result) {
    return (
      <Dialog open title="Import Postman" onClose={onClose} cancelLabel="Close">
        <p>{summaryLine(result)}</p>
        <ImportReport report={result.report} />
      </Dialog>
    );
  }

  if (!preview || !choices) {
    return (
      <Dialog
        open
        title="Import Postman"
        onClose={close}
        actions={<Button label="Choose files…" disabled={busy} onClick={() => void pick()} />}
      >
        <p>Import Postman collections (v2.0 or v2.1), environments and globals. Nothing is written until you confirm.</p>
      </Dialog>
    );
  }

  const blocker = importBlocker(preview, choices);
  const set = (patch: Partial<ImportChoices>) => setChoices({ ...choices, ...patch });
  const secrets = secretCandidates(preview);
  return (
    <Dialog
      open
      title="Import Postman"
      onClose={close}
      actions={<Button label="Import" disabled={busy || blocker !== null} onClick={() => void commit()} />}
    >
      <ul aria-label="Files">
        {preview.files.map((f) => (
          <li key={f.file}>
            <strong>{f.name}</strong> ({f.file}, {f.kind}): {f.requests} requests, {f.folders} folders, {f.variables}{" "}
            variables, {f.scripts} scripts
          </li>
        ))}
      </ul>
      <label>
        Import into
        <select
          aria-label="Import into"
          value={choices.target}
          onChange={(e) => set({ target: e.target.value === "collection" ? "collection" : "new_collection" })}
        >
          <option value="new_collection" disabled={needsCollection(preview)}>
            New collection
          </option>
          <option value="collection" disabled={trees.length === 0}>
            An open collection
          </option>
        </select>
      </label>
      {choices.target === "collection" ? (
        <label>
          Collection
          <select
            aria-label="Collection"
            value={choices.collectionId ?? ""}
            onChange={(e) => set({ collectionId: e.target.value })}
          >
            {trees.map((t) => (
              <option key={t.collection.id} value={t.collection.id}>
                {t.collection.name}
              </option>
            ))}
          </select>
        </label>
      ) : null}
      {secrets.length > 0 ? (
        <fieldset>
          <legend>Secret variables</legend>
          {secrets.map((name) => (
            <label key={name}>
              <input
                type="checkbox"
                checked={choices.secretNames.includes(name)}
                onChange={() => set({ secretNames: toggleName(choices.secretNames, name) })}
              />
              {name}
            </label>
          ))}
          <label>
            <input
              type="checkbox"
              checked={choices.storeSecretValues}
              onChange={(e) => set({ storeSecretValues: e.target.checked })}
            />
            Store secret values on this machine
          </label>
        </fieldset>
      ) : null}
      <label>
        <input
          type="checkbox"
          checked={choices.importScripts}
          onChange={(e) => set({ importScripts: e.target.checked })}
        />
        Import scripts
      </label>
      <p>{SCRIPTS_NOTE}</p>
      {hasGlobals(preview) ? (
        <label>
          Existing globals
          <select
            aria-label="Existing globals"
            value={choices.globalsClash}
            onChange={(e) => set({ globalsClash: e.target.value === "overwrite" ? "overwrite" : "keep" })}
          >
            <option value="keep">Keep mine</option>
            <option value="overwrite">Overwrite with imported</option>
          </select>
        </label>
      ) : null}
      {blocker ? <p role="status">{blocker}</p> : null}
      <ImportReport report={preview.report} truncated={preview.truncated} />
    </Dialog>
  );
}

function summaryLine(result: PostmanImportResult): string {
  const parts = [
    result.collection_id ? "collection imported" : null,
    result.environments.length > 0 ? `${result.environments.length} environments` : null,
    result.globals > 0 ? `${result.globals} globals` : null,
  ].filter(Boolean);
  return `Imported: ${parts.length > 0 ? parts.join(", ") : "nothing new"}.`;
}

/** Brings the sidebar and the environment list up to date and expands the new collection. */
async function refreshStores(result: PostmanImportResult): Promise<void> {
  await useCollectionsStore.getState().load();
  if (result.collection_id) useCollectionsStore.getState().expand(expandKey(result.collection_id, ""));
  const ids = useCollectionsStore.getState().trees.map((t) => t.collection.id);
  await Promise.all(ids.map((id) => useEnvStore.getState().loadEnvironments(id)));
}
