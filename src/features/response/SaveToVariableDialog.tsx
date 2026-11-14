import { useId, useState, type FormEvent } from "react";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import type { JsonPath } from "./JsonTree";
import { saveToVariableSnippet, type VariableScope } from "./saveToVariable";

export interface SaveToVariableDialogProps {
  readonly path: JsonPath;
  readonly onInsert: (line: string) => void;
  readonly onClose: () => void;
}

const SCOPES: readonly { readonly id: VariableScope; readonly label: string }[] = [
  { id: "env", label: "Environment" },
  { id: "collection", label: "Collection" },
  { id: "global", label: "Global" },
];

/** Asks for a variable name and scope, then hands the generated script line to `onInsert`. */
export function SaveToVariableDialog({ path, onInsert, onClose }: SaveToVariableDialogProps) {
  const formId = useId();
  const [name, setName] = useState("");
  const [scope, setScope] = useState<VariableScope>("env");

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const trimmed = name.trim();
    if (trimmed === "") return;
    onInsert(saveToVariableSnippet(path, trimmed, scope));
    onClose();
  };

  return (
    <Dialog
      open
      title="Save to variable"
      onClose={onClose}
      actions={<Button type="submit" form={formId} label="Insert" disabled={name.trim() === ""} />}
    >
      <form id={formId} onSubmit={submit} className="form-field">
        <label>
          <span>Variable name</span>
          <input value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <label>
          <span>Scope</span>
          <select aria-label="Scope" value={scope} onChange={(e) => setScope(e.target.value as VariableScope)}>
            {SCOPES.map((s) => (
              <option key={s.id} value={s.id}>
                {s.label}
              </option>
            ))}
          </select>
        </label>
      </form>
    </Dialog>
  );
}
