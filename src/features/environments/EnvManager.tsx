import { useState } from "react";
import type { VarScope } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { ConfirmDialog } from "../collections/ConfirmDialog";
import { NameDialog } from "../collections/NameDialog";
import { useEnvStore } from "./envStore";
import { VariableTable } from "./VariableTable";
import { useScopeRows } from "./useScopeRows";
import "./environments.css";

export interface EnvManagerProps {
  /** The active tab's collection; `null` for scratch tabs (globals only). */
  readonly collectionId: string | null;
  readonly onClose: () => void;
}

type Selection =
  | { readonly kind: "global" }
  | { readonly kind: "collection" }
  | { readonly kind: "environment"; readonly env: string };

type Prompt =
  | { readonly kind: "new" }
  | { readonly kind: "rename"; readonly env: string; readonly name: string }
  | { readonly kind: "delete"; readonly env: string; readonly name: string }
  | { readonly kind: "reset" }
  | { readonly kind: "discard"; readonly then: () => void };

const sameSelection = (a: Selection, b: Selection): boolean =>
  a.kind === b.kind && (a.kind !== "environment" || (b.kind === "environment" && a.env === b.env));

function scopeOf(sel: Selection, collectionId: string | null): VarScope | null {
  if (sel.kind === "global") return { scope: "global" };
  if (!collectionId) return null;
  return sel.kind === "collection"
    ? { scope: "collection", collection_id: collectionId }
    : { scope: "environment", collection_id: collectionId, env: sel.env };
}

export function EnvManager({ collectionId, onClose }: EnvManagerProps) {
  const [selection, setSelection] = useState<Selection>({ kind: "global" });
  const [prompt, setPrompt] = useState<Prompt | null>(null);
  const store = useEnvStore();
  const environments = (collectionId ? store.byCollection[collectionId]?.environments : undefined) ?? [];
  const scope = scopeOf(selection, collectionId);
  const { rows, errors, loading, dirty, setRows, save } = useScopeRows(scope);
  const storage = store.storage;

  /** Runs `action` now, or after the user agrees to drop unsaved edits. */
  const guarded = (action: () => void) => (dirty ? setPrompt({ kind: "discard", then: action }) : action());
  const choose = (next: Selection) => {
    if (!sameSelection(next, selection)) guarded(() => setSelection(next));
  };
  const closePrompt = () => setPrompt(null);

  const selectedEnv = selection.kind === "environment" ? environments.find((e) => e.env === selection.env) : undefined;

  const submitName = async (name: string, rename: boolean): Promise<boolean> => {
    if (!collectionId) return false;
    if (rename && selection.kind === "environment") {
      return store.renameEnvironment(collectionId, selection.env, name);
    }
    const before = new Set(environments.map((e) => e.env));
    const ok = await store.createEnvironment(collectionId, name);
    const created = useEnvStore.getState().byCollection[collectionId]?.environments.find((e) => !before.has(e.env));
    if (ok && created) setSelection({ kind: "environment", env: created.env });
    return ok;
  };

  const confirmDelete = async (env: string) => {
    closePrompt();
    if (collectionId && (await store.deleteEnvironment(collectionId, env))) setSelection({ kind: "collection" });
  };

  return (
    <>
      <Dialog
        open
        title="Variables"
        onClose={() => guarded(onClose)}
        actions={<Button label="Save" disabled={!dirty || loading} onClick={() => void save()} />}
      >
        {storage && storage.mode !== "keychain" ? (
          <div className="env-storage-warning" role="status">
            <span>{storage.message ?? "Secrets are not stored in the system keychain."}</span>
            {/* Only a damaged vault may be reset: in session-only mode the vault file is intact. */}
            {storage.mode === "damaged" ? (
              <Button label="Reset secret storage" onClick={() => setPrompt({ kind: "reset" })} />
            ) : null}
          </div>
        ) : null}
        <div className="env-manager">
          <nav className="env-scopes" aria-label="Variable scopes">
            <Button label="Globals" pressed={selection.kind === "global"} onClick={() => choose({ kind: "global" })} />
            {collectionId ? (
              <>
                <Button
                  label="Collection variables"
                  pressed={selection.kind === "collection"}
                  onClick={() => choose({ kind: "collection" })}
                />
                {environments.map((env) => (
                  <Button
                    key={env.env}
                    label={env.invalid ? `${env.name} (invalid)` : env.name}
                    pressed={selection.kind === "environment" && selection.env === env.env}
                    onClick={() => choose({ kind: "environment", env: env.env })}
                  />
                ))}
                <Button label="New environment" onClick={() => setPrompt({ kind: "new" })} />
                {selectedEnv ? (
                  <>
                    <Button
                      label="Rename"
                      disabled={selectedEnv.invalid}
                      onClick={() => setPrompt({ kind: "rename", env: selectedEnv.env, name: selectedEnv.name })}
                    />
                    <Button
                      label="Delete"
                      onClick={() => setPrompt({ kind: "delete", env: selectedEnv.env, name: selectedEnv.name })}
                    />
                  </>
                ) : null}
              </>
            ) : (
              <p className="hint">Open a collection request to edit collection variables and environments.</p>
            )}
          </nav>
          <div className="env-editor">
            {selectedEnv?.invalid ? (
              <p className="field-error">This environment file could not be read. It can only be deleted.</p>
            ) : loading ? (
              <p className="hint">Loading...</p>
            ) : (
              <VariableTable rows={rows} errors={errors} showCurrent={selection.kind !== "global"} onChange={setRows} />
            )}
          </div>
        </div>
      </Dialog>
      {prompt?.kind === "new" ? (
        <NameDialog title="New Environment" submitLabel="Create" onSubmit={(n) => submitName(n, false)} onClose={closePrompt} />
      ) : null}
      {prompt?.kind === "rename" ? (
        <NameDialog
          title="Rename Environment"
          initialName={prompt.name}
          submitLabel="Rename"
          onSubmit={(n) => submitName(n, true)}
          onClose={closePrompt}
        />
      ) : null}
      {prompt?.kind === "delete" ? (
        <ConfirmDialog
          title="Delete environment?"
          message={`Delete "${prompt.name}"? Its local values and secrets are removed too.`}
          onConfirm={() => void confirmDelete(prompt.env)}
          onClose={closePrompt}
        />
      ) : null}
      {prompt?.kind === "reset" ? (
        <ConfirmDialog
          title="Reset secret storage?"
          message="All stored secret values on this machine are removed."
          confirmLabel="Reset"
          onConfirm={() => {
            closePrompt();
            void store.resetStorage();
          }}
          onClose={closePrompt}
        />
      ) : null}
      {prompt?.kind === "discard" ? (
        <ConfirmDialog
          title="Discard changes?"
          message="You have unsaved changes to these variables."
          confirmLabel="Discard"
          onConfirm={() => {
            closePrompt();
            prompt.then();
          }}
          onClose={closePrompt}
        />
      ) : null}
    </>
  );
}
