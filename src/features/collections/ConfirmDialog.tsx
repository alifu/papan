import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";

export interface ConfirmDialogProps {
  readonly title: string;
  readonly message: string;
  readonly confirmLabel?: string;
  readonly onConfirm: () => void;
  readonly onClose: () => void;
}

/** Modal yes/no question. Mount it only while it is shown. */
export function ConfirmDialog({
  title,
  message,
  confirmLabel = "Delete",
  onConfirm,
  onClose,
}: ConfirmDialogProps) {
  return (
    <Dialog
      open
      title={title}
      onClose={onClose}
      actions={<Button label={confirmLabel} onClick={onConfirm} />}
    >
      <p className="dialog-message">{message}</p>
    </Dialog>
  );
}
