import { useState } from "react";
import { z } from "zod";
import type { Settings } from "../../ipc/bindings";

export interface SettingsEditorProps {
  readonly settings: Settings;
  readonly onChange: (settings: Settings) => void;
}

export const MAX_TIMEOUT_MS = 600000;
const timeoutSchema = z.coerce.number().int().min(0).max(MAX_TIMEOUT_MS);
export const TIMEOUT_ERROR = `Timeout must be a whole number from 0 to ${MAX_TIMEOUT_MS} ms.`;

export function SettingsEditor({ settings, onChange }: SettingsEditorProps) {
  const [text, setText] = useState(String(settings.timeout_ms));
  const [error, setError] = useState<string | null>(null);

  const editTimeout = (next: string) => {
    setText(next);
    const parsed = next.trim() === "" ? null : timeoutSchema.safeParse(next);
    if (parsed?.success) {
      setError(null);
      onChange({ ...settings, timeout_ms: parsed.data });
    } else {
      setError(TIMEOUT_ERROR);
    }
  };

  return (
    <div className="editor-form">
      <label>
        <input
          type="checkbox"
          checked={settings.follow_redirects}
          onChange={(e) => onChange({ ...settings, follow_redirects: e.target.checked })}
        />
        Follow redirects
      </label>
      <label>
        <input
          type="checkbox"
          checked={settings.verify_tls}
          onChange={(e) => onChange({ ...settings, verify_tls: e.target.checked })}
        />
        Verify TLS certificates
      </label>
      <label>
        Timeout (ms, 0 = none)
        <input
          inputMode="numeric"
          aria-invalid={error !== null}
          value={text}
          onChange={(e) => editTimeout(e.target.value)}
        />
      </label>
      {error ? <p role="alert" className="field-error">{error}</p> : null}
    </div>
  );
}
