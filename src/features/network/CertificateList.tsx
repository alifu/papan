import { useState } from "react";
import type { ClientCertificate } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { ConfirmDialog } from "../collections/ConfirmDialog";

export interface CertificateListProps {
  readonly certificates: readonly ClientCertificate[];
  readonly onAdd: (host: string, port: number | null) => Promise<boolean>;
  readonly onRemove: (id: string) => Promise<boolean>;
}

const MAX_PORT = 65535;
const HOST_ERROR = "Enter a host name.";
const PORT_ERROR = "Port must be empty or 1 to 65535.";

export const formatAdded = (ms: number | null): string =>
  ms === null ? "" : new Date(ms).toISOString().slice(0, 10);

/** Port text to a port; empty means any port, `undefined` is invalid. */
export function parsePort(text: string): number | null | undefined {
  if (text.trim() === "") return null;
  const n = Number(text);
  return Number.isInteger(n) && n >= 1 && n <= MAX_PORT ? n : undefined;
}

export function CertificateList({ certificates, onAdd, onRemove }: CertificateListProps) {
  const [host, setHost] = useState("");
  const [portText, setPortText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [removing, setRemoving] = useState<ClientCertificate | null>(null);

  const add = async () => {
    const port = parsePort(portText);
    if (host.trim() === "") return setError(HOST_ERROR);
    if (port === undefined) return setError(PORT_ERROR);
    setError(null);
    if (await onAdd(host.trim(), port)) {
      setHost("");
      setPortText("");
    }
  };

  return (
    <fieldset className="editor-form">
      <legend>Client certificates</legend>
      {certificates.length === 0 ? (
        <p className="hint">No client certificates.</p>
      ) : (
        <table className="response-table" aria-label="Client certificates">
          <thead>
            <tr>
              <th>Host</th>
              <th>Port</th>
              <th>Files</th>
              <th>Added</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {certificates.map((c) => (
              <tr key={c.id}>
                <td>{c.host}</td>
                <td>{c.port ?? "any"}</td>
                <td>{c.missing ? <span className="field-error">Missing. Import it again.</span> : `${c.cert_name}, ${c.key_name}`}</td>
                <td>{formatAdded(c.added_at_ms)}</td>
                <td>
                  <Button label="Remove" aria-label={`Remove certificate ${c.host}`} onClick={() => setRemoving(c)} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <label>
        Host
        <input spellCheck={false} value={host} onChange={(e) => setHost(e.target.value)} />
      </label>
      <label>
        Port (empty = any)
        <input inputMode="numeric" value={portText} onChange={(e) => setPortText(e.target.value)} />
      </label>
      {error ? <p role="alert" className="field-error">{error}</p> : null}
      <Button label="Add…" onClick={() => void add()} />
      {removing ? (
        <ConfirmDialog
          title="Remove certificate?"
          message={`Remove the client certificate for ${removing.host}? It is deleted from secret storage.`}
          confirmLabel="Remove"
          onConfirm={() => {
            const id = removing.id;
            setRemoving(null);
            void onRemove(id);
          }}
          onClose={() => setRemoving(null)}
        />
      ) : null}
    </fieldset>
  );
}
