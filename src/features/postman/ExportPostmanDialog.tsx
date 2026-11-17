import { useEffect, useRef, useState } from "react";
import type { EnvironmentSummary, PostmanExportResult } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { reportError } from "../../ui/toastStore";
import { useCollectionsStore } from "../collections/collectionsStore";
import * as envApi from "../environments/api";
import * as api from "./api";
import { ImportReport } from "./ImportReport";
import { toggleName } from "./postmanModel";

export interface ExportPostmanDialogProps {
  readonly collectionId: string | null;
  readonly onClose: () => void;
}

export const SECRET_WARNING =
  "Secret values from this machine's vault will be written into the exported file in plain text. Anyone with the file can read them.";

/** File > Export to Postman... or the collection row menu: writes v2.1 JSON files into a folder Rust asks for. */
export function ExportPostmanDialog({ collectionId, onClose }: ExportPostmanDialogProps) {
  const trees = useCollectionsStore((s) => s.trees);
  const [selected, setSelected] = useState<string | null>(collectionId ?? trees[0]?.collection.id ?? null);
  const [envs, setEnvs] = useState<readonly EnvironmentSummary[]>([]);
  const [chosenEnvs, setChosenEnvs] = useState<readonly string[]>([]);
  const [globals, setGlobals] = useState(false);
  const [secrets, setSecrets] = useState(false);
  const [result, setResult] = useState<PostmanExportResult | null>(null);
  const [busy, setBusy] = useState(false);
  const closed = useRef(false);

  useEffect(() => {
    closed.current = false;
    return () => {
      closed.current = true;
    };
  }, []);

  // A stale answer for a collection that is no longer selected is ignored.
  useEffect(() => {
    let current = true;
    setEnvs([]);
    setChosenEnvs([]);
    if (selected) {
      envApi
        .listEnvironments(selected)
        .then((list) => current && setEnvs(list.environments))
        .catch((err) => current && reportError(err));
    }
    return () => {
      current = false;
    };
  }, [selected]);

  const run = async () => {
    if (!selected) return;
    setBusy(true);
    try {
      const done = await api.exportCollection({
        collection_id: selected,
        environments: [...chosenEnvs],
        include_globals: globals,
        include_secret_values: secrets,
      });
      if (done && !closed.current) setResult(done); // null: the user cancelled the folder dialog
    } catch (err) {
      reportError(err);
    } finally {
      if (!closed.current) setBusy(false);
    }
  };

  if (result) {
    return (
      <Dialog open title="Export to Postman" onClose={onClose} cancelLabel="Close">
        <p>Written files:</p>
        <ul aria-label="Written files">
          {result.files.map((name) => (
            <li key={name}>{name}</li>
          ))}
        </ul>
        <ImportReport report={result.report} />
      </Dialog>
    );
  }

  return (
    <Dialog
      open
      title="Export to Postman"
      onClose={onClose}
      actions={<Button label="Export" disabled={busy || selected === null} onClick={() => void run()} />}
    >
      <label>
        Collection
        <select aria-label="Collection" value={selected ?? ""} onChange={(e) => setSelected(e.target.value || null)}>
          {trees.map((t) => (
            <option key={t.collection.id} value={t.collection.id}>
              {t.collection.name}
            </option>
          ))}
        </select>
      </label>
      {trees.length === 0 ? <p>Open a collection first.</p> : null}
      {envs.length > 0 ? (
        <fieldset>
          <legend>Environments</legend>
          {envs.map((env) => (
            <label key={env.env}>
              <input
                type="checkbox"
                checked={chosenEnvs.includes(env.env)}
                onChange={() => setChosenEnvs(toggleName(chosenEnvs, env.env))}
              />
              {env.name}
            </label>
          ))}
        </fieldset>
      ) : null}
      <label>
        <input type="checkbox" checked={globals} onChange={(e) => setGlobals(e.target.checked)} />
        Include globals
      </label>
      <label>
        <input type="checkbox" checked={secrets} onChange={(e) => setSecrets(e.target.checked)} />
        Include secret values
      </label>
      {secrets ? <p role="alert">{SECRET_WARNING}</p> : <p>Secret values are left blank in the export.</p>}
    </Dialog>
  );
}
