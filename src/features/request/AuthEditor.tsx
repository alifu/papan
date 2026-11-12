import { useState } from "react";
import type { ApiKeyPlacement, Auth, EffectiveVariable, RequestLocation } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { SaveSecretDialog } from "../environments/SaveSecretDialog";
import { findTokens } from "../environments/template";
import { unresolvedAttrs } from "../environments/unresolvedAttrs";

export interface AuthEditorProps {
  readonly auth: Auth;
  readonly onChange: (auth: Auth) => void;
  /** Enables "Save as secret variable…"; omitted when there is nowhere to save. */
  readonly location?: RequestLocation;
  readonly variables?: readonly EffectiveVariable[];
}

export const SECRET_HINT = "Kept until you quit. Not written to the collection file.";

const DEFAULTS: Record<string, Auth> = {
  none: { type: "none" },
  basic: { type: "basic", username: "", password: "" },
  bearer: { type: "bearer", token: "" },
  api_key: { type: "api_key", name: "", value: "", placement: "header" },
};

interface FieldProps {
  readonly label: string;
  readonly value: string;
  readonly onChange: (v: string) => void;
  readonly variables?: readonly EffectiveVariable[];
}

interface SecretProps extends FieldProps {
  readonly location?: RequestLocation;
  readonly defaultName: string;
}

const isSingleReference = (text: string): boolean => {
  const trimmed = text.trim();
  const tokens = findTokens(trimmed);
  return tokens.length === 1 && tokens[0]?.from === 0 && tokens[0].to === trimmed.length;
};

function Secret({ label, value, onChange, variables, location, defaultName }: SecretProps) {
  const [shown, setShown] = useState(false);
  const [saving, setSaving] = useState(false);
  const canSave = location !== undefined && value !== "" && !isSingleReference(value);
  return (
    <div className="field">
      <label>
        {label}
        <input
          type={shown ? "text" : "password"}
          spellCheck={false}
          {...unresolvedAttrs(value, variables)}
          value={value}
          onChange={(e) => onChange(e.target.value)}
        />
      </label>
      <label>
        <input type="checkbox" checked={shown} onChange={(e) => setShown(e.target.checked)} />
        Show {label.toLowerCase()}
      </label>
      <p className="hint">{SECRET_HINT}</p>
      {canSave ? <Button label="Save as secret variable…" onClick={() => setSaving(true)} /> : null}
      {saving && location ? (
        <SaveSecretDialog
          location={location}
          secret={value}
          defaultName={defaultName}
          onSaved={onChange}
          onClose={() => setSaving(false)}
        />
      ) : null}
    </div>
  );
}

function Plain({ label, value, onChange, variables }: FieldProps) {
  return (
    <div className="field">
      <label>
        {label}
        <input spellCheck={false} {...unresolvedAttrs(value, variables)} value={value} onChange={(e) => onChange(e.target.value)} />
      </label>
    </div>
  );
}

export function AuthEditor({ auth, onChange, location, variables }: AuthEditorProps) {
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
          <Plain label="Username" value={auth.username} variables={variables} onChange={(username) => onChange({ ...auth, username })} />
          <Secret
            label="Password"
            value={auth.password}
            variables={variables}
            location={location}
            defaultName="password"
            onChange={(password) => onChange({ ...auth, password })}
          />
        </>
      ) : null}
      {auth.type === "bearer" ? (
        <Secret
          label="Token"
          value={auth.token}
          variables={variables}
          location={location}
          defaultName="token"
          onChange={(token) => onChange({ ...auth, token })}
        />
      ) : null}
      {auth.type === "api_key" ? (
        <>
          <Plain label="Key name" value={auth.name} variables={variables} onChange={(name) => onChange({ ...auth, name })} />
          <Secret
            label="Key value"
            value={auth.value}
            variables={variables}
            location={location}
            defaultName="api_key"
            onChange={(value) => onChange({ ...auth, value })}
          />
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
