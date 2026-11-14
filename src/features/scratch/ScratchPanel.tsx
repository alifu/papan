import { useState } from "react";
import type { ScratchItem } from "../../ipc/bindings";
import { useTabsStore } from "../../shell/tabsStore";
import { Icon } from "../../ui/icons";
import { Menu } from "../../ui/Menu";
import { NameDialog } from "../collections/NameDialog";
import { kindBadge, kindIcon, REQUEST_KIND_OPTIONS } from "../collections/requestKind";
import { scratchLocation } from "../locations";
import { MoveToCollectionDialog } from "./MoveToCollectionDialog";
import { useScratchStore } from "./scratchStore";
import "../collections/collections.css";

type DialogState =
  | { readonly kind: "rename"; readonly item: ScratchItem }
  | { readonly kind: "move"; readonly item: ScratchItem };

const EMPTY_TEXT = "No scratch requests yet.";

export function ScratchPanel() {
  const items = useScratchStore((s) => s.items);
  const [dialog, setDialog] = useState<DialogState | null>(null);
  const closeDialog = () => setDialog(null);

  return (
    <div className="scratch-panel">
      <div className="panel-actions">
        <Menu
          label="New Request"
          items={REQUEST_KIND_OPTIONS.map(({ kind, label }) => ({
            label,
            onSelect: () => void useScratchStore.getState().create(kind),
          }))}
        />
      </div>
      {items.length === 0 ? (
        <p className="empty-text">{EMPTY_TEXT}</p>
      ) : (
        <ul className="scratch-list" aria-label="Scratch requests">
          {items.map((item) => (
            <li key={item.id} className="tree-row">
              <button
                type="button"
                className="scratch-open"
                onClick={() => useTabsStore.getState().openRequest(scratchLocation(item.id), item.name)}
              >
                <Icon name={kindIcon(item.kind)} />
                <span className="tree-label">{item.name}</span>
                <span className="tree-method">{kindBadge(item.kind, item.method)}</span>
              </button>
              <Menu
                label={`Actions for ${item.name}`}
                icon="chevron-down"
                className="tree-menu"
                items={[
                  { label: "Rename", onSelect: () => setDialog({ kind: "rename", item }) },
                  { label: "Move to collection…", onSelect: () => setDialog({ kind: "move", item }) },
                  { label: "Delete", onSelect: () => void useScratchStore.getState().remove(item.id) },
                ]}
              />
            </li>
          ))}
        </ul>
      )}
      {dialog?.kind === "rename" ? (
        <NameDialog
          title="Rename"
          initialName={dialog.item.name}
          submitLabel="Rename"
          onSubmit={(name) => useScratchStore.getState().rename(dialog.item.id, name)}
          onClose={closeDialog}
        />
      ) : null}
      {dialog?.kind === "move" ? (
        <MoveToCollectionDialog item={dialog.item} onClose={closeDialog} />
      ) : null}
    </div>
  );
}
