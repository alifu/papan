import { useRef, useState, type FormEvent } from "react";
import type { RequestLocation, WsdlImport, WsdlSource } from "../../ipc/bindings";
import { isAppError, toUserMessage } from "../../ipc/errors";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import * as api from "./api";

export interface WsdlImportDialogProps {
  readonly location: RequestLocation;
  /** Called with the parsed WSDL when the user accepts the result. */
  readonly onImported: (wsdl: WsdlImport) => void;
  readonly onClose: () => void;
}

const countOperations = (wsdl: WsdlImport): number =>
  wsdl.services.reduce((sum, s) => sum + s.ports.reduce((n, p) => n + p.operations.length, 0), 0);

/** Imports a WSDL from a URL or a picked file and lists what was fetched and skipped. */
export function WsdlImportDialog({ location, onImported, onClose }: WsdlImportDialogProps) {
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<WsdlImport | null>(null);
  const importId = useRef<string | null>(null);

  const run = async (source: WsdlSource) => {
    const id = crypto.randomUUID();
    importId.current = id;
    setBusy(true);
    setError(null);
    try {
      const imported = await api.importWsdl(id, location, source);
      if (imported) setResult(imported);
    } catch (err) {
      if (!(isAppError(err) && err.code === "cancelled")) setError(toUserMessage(err));
    } finally {
      importId.current = null;
      setBusy(false);
    }
  };

  const submitUrl = (event: FormEvent) => {
    event.preventDefault();
    if (url.trim() === "") {
      setError("Enter a WSDL URL or pick a file.");
      return;
    }
    void run({ type: "url", url: url.trim() });
  };

  const cancel = () => {
    if (importId.current) void api.cancelImport(importId.current);
  };

  return (
    <Dialog
      open
      title="Import WSDL"
      onClose={onClose}
      actions={
        result ? (
          <Button
            label="Use this WSDL"
            onClick={() => {
              onImported(result);
              onClose();
            }}
          />
        ) : undefined
      }
    >
      <form className="form-field" onSubmit={submitUrl}>
        <label>
          <span>WSDL URL</span>
          <input
            value={url}
            disabled={busy}
            placeholder="https://example.com/service?wsdl"
            onChange={(event) => setUrl(event.target.value)}
          />
        </label>
        <div className="soap-actions">
          <Button type="submit" label="Import" disabled={busy} />
          <Button label="From file…" disabled={busy} onClick={() => void run({ type: "pick_file" })} />
          {busy ? <Button label="Stop import" onClick={cancel} /> : null}
        </div>
      </form>
      {busy ? <p role="status">Importing…</p> : null}
      {error ? <p className="field-error" role="alert">{error}</p> : null}
      {result ? <ImportResult wsdl={result} /> : null}
    </Dialog>
  );
}

function ImportResult({ wsdl }: { readonly wsdl: WsdlImport }) {
  return (
    <div className="soap-result">
      <p>
        {wsdl.services.length} service(s), {countOperations(wsdl)} operation(s) from {wsdl.source}
      </p>
      {wsdl.fetched.length > 0 ? (
        <>
          <h4>Fetched</h4>
          <ul aria-label="Fetched documents">
            {wsdl.fetched.map((location) => (
              <li key={location}>{location}</li>
            ))}
          </ul>
        </>
      ) : null}
      {wsdl.warnings.length > 0 ? (
        <>
          <h4>Warnings</h4>
          <ul aria-label="Warnings">
            {wsdl.warnings.map((warning, index) => (
              <li key={`${index}-${warning}`}>{warning}</li>
            ))}
          </ul>
        </>
      ) : null}
    </div>
  );
}
