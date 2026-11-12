import { useState } from "react";
import type { ApiKeyPlacement, Auth } from "../../ipc/bindings";

export interface AuthEditorProps {
  readonly auth: Auth;
  readonly onChange: (auth: Auth) => void;
}

export const SECRET_HINT = "Kept until you quit. Not written to the collection file.";

const DEFAULTS: Record<string, Auth> = {
  none: { type: "none" },
  basic: { type: "basic", username: "", password: "" },
  bearer: { type: "bearer", token: "" },
  api_key: { type: "api_key", name: "", value: "", placement: "header" },
};

function Secret({ label, value, onChange }: { label: string; value: string; onChange: (v: string) => void }) {
  const [shown, setShown] = useState(false);
  return (
    <div className="field">
      <label>
        {label}
        <input type={shown ? "text" : "password"} spellCheck={false} value={value} onChange={(e) => onChange(e.target.value)} />
      </label>
      <label>
        <input type="checkbox" checked={shown} onChange={(e) => setShown(e.target.checked)} />
        Show {label.toLowerCase()}
      </label>
      <p className="hint">{SECRET_HINT}</p>
    </div>
  );
}

function Plain({ label, value, onChange }: { label: string; value: string; onChange: (v: string) => void }) {
  return (
    <div className="field">
      <label>
        {label}
        <input spellCheck={false} value={value} onChange={(e) => onChange(e.target.value)} />
      </label>
    </div>
  );
}

export function AuthEditor({ auth, onChange }: AuthEditorProps) {
  if (auth.type === "unsupported") {
    return <p className="empty-text">Made by a newer Papan. This authentication is read-only.</p>;
  }
  return (
    <div className="editor-form">
      <label>
        Type
        <select
          aria-label="Auth type"
          value={auth.type}
          onChange={(e) => onChange(DEFAULTS[e.target.value] ?? DEFAULTS.none)}
        >
          <option value="none">None</option>
          <option value="basic">Basic</option>
          <option value="bearer">Bearer token</option>
          <option value="api_key">API key</option>
        </select>
      </label>
      {auth.type === "basic" ? (
        <>
          <Plain label="Username" value={auth.username} onChange={(username) => onChange({ ...auth, username })} />
          <Secret label="Password" value={auth.password} onChange={(password) => onChange({ ...auth, password })} />
        </>
      ) : null}
      {auth.type === "bearer" ? (
        <Secret label="Token" value={auth.token} onChange={(token) => onChange({ ...auth, token })} />
      ) : null}
      {auth.type === "api_key" ? (
        <>
          <Plain label="Key name" value={auth.name} onChange={(name) => onChange({ ...auth, name })} />
          <Secret label="Key value" value={auth.value} onChange={(value) => onChange({ ...auth, value })} />
          <label>
            Add to
            <select
              aria-label="Add to"
              value={auth.placement}
              onChange={(e) => onChange({ ...auth, placement: e.target.value as ApiKeyPlacement })}
            >
              <option value="header">Header</option>
              <option value="query">Query parameter</option>
            </select>
          </label>
        </>
      ) : null}
    </div>
  );
}
