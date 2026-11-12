import { ConfirmDialog } from "../features/collections/ConfirmDialog";
import { useTabsStore } from "./tabsStore";

/** Asks before closing a tab with unsaved changes. */
export function DiscardDialog() {
  const pendingCloseId = useTabsStore((s) => s.pendingCloseId);
  const title = useTabsStore((s) => s.tabs.find((t) => t.id === s.pendingCloseId)?.title);
  const discardTab = useTabsStore((s) => s.discardTab);
  const cancelClose = useTabsStore((s) => s.cancelClose);
  if (pendingCloseId === null) return null;
  return (
    <ConfirmDialog
      title="Discard changes?"
      message={`"${title ?? "This request"}" has unsaved changes.`}
      confirmLabel="Discard"
      onConfirm={() => discardTab(pendingCloseId)}
      onClose={cancelClose}
    />
  );
}
