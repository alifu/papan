import { useState } from "react";
import type { EffectiveVariable, RequestLocation } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { SaveSecretDialog } from "../environments/SaveSecretDialog";
import { findTokens } from "../environments/template";
import { unresolvedAttrs } from "../environments/unresolvedAttrs";

export const SECRET_HINT = "Kept until you quit. Not written to the collection file.";

export interface FieldProps {
  readonly label: string;
  readonly value: string;
  readonly onChange: (v: string) => void;
  readonly variables?: readonly EffectiveVariable[];
}

export interface SecretProps extends FieldProps {
  readonly location?: RequestLocation;
  readonly defaultName: string;
}

const isSingleReference = (text: string): boolean => {
  const trimmed = text.trim();
  const tokens = findTokens(trimmed);
  return tokens.length === 1 && tokens[0]?.from === 0 && tokens[0].to === trimmed.length;
};

export function Secret({ label, value, onChange, variables, location, defaultName }: SecretProps) {
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

export function Plain({ label, value, onChange, variables }: FieldProps) {
  return (
    <div className="field">
      <label>
        {label}
        <input spellCheck={false} {...unresolvedAttrs(value, variables)} value={value} onChange={(e) => onChange(e.target.value)} />
      </label>
    </div>
  );
}
