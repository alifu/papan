import { useId, useState, type FormEvent } from "react";
import type { RequestLocation, VarScope } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { reportError } from "../../ui/toastStore";
import { readVariables, writeVariables } from "./api";
import { useEnvStore } from "./envStore";
import { nameSchema, rowFromVariable, toInput } from "./variableRows";

export interface SaveSecretDialogProps {
  readonly location: RequestLocation;
  /** The literal secret typed in the field. */
  readonly secret: string;
  readonly defaultName: string;
  /** Called with `{{name}}` once the secret is stored. */
  readonly onSaved: (reference: string) => void;
  readonly onClose: () => void;
}

/** Where a new variable goes: the selected environment, else the collection, else globals for scratch. */
export function targetScope(location: RequestLocation): { scope: VarScope; label: string } {
  if (location.scope === "scratch") return { scope: { scope: "global" }, label: "Globals" };
  const selected = useEnvStore.getState().byCollection[location.collection_id]?.selected ?? null;
  if (selected === null) {
    return { scope: { scope: "collection", collection_id: location.collection_id }, label: "Collection variables" };
  }
  const name = useEnvStore.getState().byCollection[location.collection_id]?.environments.find((e) => e.env === selected)?.name;
  return {
    scope: { scope: "environment", collection_id: location.collection_id, env: selected },
    label: `Environment "${name ?? selected}"`,
  };
}

/** Moves a literal secret into a secret variable and hands back the `{{name}}` reference. */
export function SaveSecretDialog({ location, secret, defaultName, onSaved, onClose }: SaveSecretDialogProps) {
  const formId = useId();
  const [name, setName] = useState(defaultName);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const target = targetScope(location);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const parsed = nameSchema.safeParse(name.trim());
    if (!parsed.success) {
      setError(parsed.error.issues[0]?.message ?? "Name is not valid.");
      return;
    }
    setBusy(true);
    try {
      const existing = await readVariables(target.scope);
      if (existing.some((v) => v.name === parsed.data)) {
        setError("A variable with that name already exists.");
        return;
      }
      await writeVariables(target.scope, [
        ...existing.map((v) => toInput(rowFromVariable(v))),
        {
          name: parsed.data,
          previous_name: null,
          value: "",
          current: null,
          enabled: true,
          secret: true,
          secret_edit: { op: "set", value: secret },
        },
      ]);
      await useEnvStore.getState().refreshEffective();
      onSaved(`{{${parsed.data}}}`);
      onClose();
    } catch (err) {
      reportError(err);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      title="Save as secret variable"
      onClose={onClose}
      actions={<Button type="submit" form={formId} label="Save" disabled={busy} />}
    >
      <form id={formId} onSubmit={(event) => void submit(event)} className="form-field">
        <label>
          <span>Variable name</span>
          <input value={name} spellCheck={false} onChange={(e) => setName(e.target.value)} aria-invalid={error !== null} />
        </label>
        <p className="hint">Stored in: {target.label}. The field will read {`{{${name.trim() || "name"}}}`}.</p>
        {error ? <p className="field-error" role="alert">{error}</p> : null}
      </form>
    </Dialog>
  );
}
