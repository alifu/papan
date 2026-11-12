import { useId, useState, type FormEvent } from "react";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { checkName } from "./nameSchema";

export interface NameDialogProps {
  readonly title: string;
  readonly initialName?: string;
  readonly submitLabel?: string;
  /** Resolves true when the dialog may close. */
  readonly onSubmit: (name: string) => Promise<boolean>;
  readonly onClose: () => void;
}

/** Modal with one validated name field. Mount it only while it is shown. */
export function NameDialog({
  title,
  initialName = "",
  submitLabel = "OK",
  onSubmit,
  onClose,
}: NameDialogProps) {
  const formId = useId();
  const [value, setValue] = useState(initialName);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const check = checkName(value);
    if (!check.ok) {
      setError(check.message);
      return;
    }
    setError(null);
    setBusy(true);
    const done = await onSubmit(check.name);
    setBusy(false);
    if (done) onClose();
  };

  return (
    <Dialog
      open
      title={title}
      onClose={onClose}
      actions={<Button type="submit" form={formId} label={submitLabel} disabled={busy} />}
    >
      <form id={formId} onSubmit={(event) => void submit(event)} className="form-field">
        <label>
          <span>Name</span>
          <input
            value={value}
            onChange={(event) => setValue(event.target.value)}
            aria-invalid={error !== null}
            aria-describedby={error ? `${formId}-error` : undefined}
          />
        </label>
        {error ? (
          <p id={`${formId}-error`} className="field-error">
            {error}
          </p>
        ) : null}
      </form>
    </Dialog>
  );
}
