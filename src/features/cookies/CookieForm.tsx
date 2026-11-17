import { useState } from "react";
import type { CookieInput, StoredCookie } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";

export interface CookieFormProps {
  /** The cookie being edited; `null` adds a new one. */
  readonly initial: StoredCookie | null;
  readonly onSubmit: (cookie: CookieInput) => Promise<boolean>;
  readonly onClose: () => void;
}

export const NAME_DOMAIN_ERROR = "Name and domain are required.";
export const EXPIRY_ERROR = "Expiry must be a date such as 2030-01-31 12:00, or empty for a session cookie.";

/** Expiry text to ms; empty is a session cookie, `undefined` is invalid. */
export function parseExpiry(text: string): number | null | undefined {
  if (text.trim() === "") return null;
  const ms = Date.parse(/Z|[+-]\d\d:?\d\d$/.test(text.trim()) ? text.trim() : `${text.trim().replace(" ", "T")}Z`);
  return Number.isNaN(ms) ? undefined : ms;
}

const toText = (ms: number | null | undefined): string =>
  ms == null ? "" : new Date(ms).toISOString().replace("T", " ").slice(0, 19);

export function CookieForm({ initial, onSubmit, onClose }: CookieFormProps) {
  const [name, setName] = useState(initial?.name ?? "");
  const [value, setValue] = useState(initial?.value ?? "");
  const [domain, setDomain] = useState(initial?.domain ?? "");
  const [path, setPath] = useState(initial?.path ?? "/");
  const [expires, setExpires] = useState(toText(initial?.expires_at_ms));
  const [httpOnly, setHttpOnly] = useState(initial?.http_only ?? false);
  const [secure, setSecure] = useState(initial?.secure ?? false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    const expires_at_ms = parseExpiry(expires);
    if (name.trim() === "" || domain.trim() === "") return setError(NAME_DOMAIN_ERROR);
    if (expires_at_ms === undefined) return setError(EXPIRY_ERROR);
    setError(null);
    const cookie = { name: name.trim(), value, domain: domain.trim(), path: path || "/", expires_at_ms, http_only: httpOnly, secure };
    if (await onSubmit(cookie)) onClose();
  };

  return (
    <Dialog
      open
      title={initial ? "Edit cookie" : "Add cookie"}
      onClose={onClose}
      actions={<Button label="Save" onClick={() => void submit()} />}
    >
      <div className="editor-form">
        <label>
          Name
          <input spellCheck={false} value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <label>
          Value
          <input spellCheck={false} value={value} onChange={(e) => setValue(e.target.value)} />
        </label>
        <label>
          Domain
          <input spellCheck={false} value={domain} onChange={(e) => setDomain(e.target.value)} />
        </label>
        <label>
          Path
          <input spellCheck={false} value={path} onChange={(e) => setPath(e.target.value)} />
        </label>
        <label>
          Expires (UTC, empty = session)
          <input spellCheck={false} value={expires} onChange={(e) => setExpires(e.target.value)} />
        </label>
        <label>
          <input type="checkbox" checked={httpOnly} onChange={(e) => setHttpOnly(e.target.checked)} />
          HttpOnly
        </label>
        <label>
          <input type="checkbox" checked={secure} onChange={(e) => setSecure(e.target.checked)} />
          Secure
        </label>
        {error ? <p role="alert" className="field-error">{error}</p> : null}
      </div>
    </Dialog>
  );
}
