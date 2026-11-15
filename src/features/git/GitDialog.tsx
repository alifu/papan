import { useCallback, useEffect, useRef, useState } from "react";
import type { ConflictSide, GitBranch, GitStatus } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { reportError, showToast } from "../../ui/toastStore";
import { useCollectionsStore } from "../collections/collectionsStore";
import * as api from "./api";
import {
  changeLabel,
  conflictsOf,
  diffLineClass,
  syncSummary,
} from "./gitModel";

export interface GitDialogProps {
  readonly collectionId: string;
  readonly onClose: () => void;
}

/** Collection row menu > Git…: status, diff, commit, pull, push, branches, conflicts. */
export function GitDialog({ collectionId, onClose }: GitDialogProps) {
  const [status, setStatus] = useState<GitStatus | null>(null);
  const [branches, setBranches] = useState<readonly GitBranch[]>([]);
  const [message, setMessage] = useState("");
  const [newBranch, setNewBranch] = useState("");
  const [selected, setSelected] = useState<{
    path: string;
    text: string;
  } | null>(null);
  const [busy, setBusy] = useState(false);
  const closed = useRef(false);
  const name = useCollectionsStore(
    (s) =>
      s.trees.find((t) => t.collection.id === collectionId)?.collection.name,
  );

  useEffect(() => {
    closed.current = false;
    return () => {
      closed.current = true;
    };
  }, []);

  const refresh = useCallback(async () => {
    try {
      const next = await api.status(collectionId);
      if (closed.current) return;
      setStatus(next);
      setBranches(next.is_repo ? await api.branches(collectionId) : []);
    } catch (err) {
      reportError(err);
    }
  }, [collectionId]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  /** Runs one git action, then reloads status and the collection tree (files may have changed). */
  const act = async (
    work: () => Promise<{ message: string } | GitStatus | void>,
    clearMessage = false,
  ) => {
    setBusy(true);
    try {
      const result = await work();
      if (result && "message" in result && typeof result.message === "string") {
        showToast({ kind: "info", message: result.message });
      }
      if (clearMessage) setMessage("");
      setSelected(null);
      await refresh();
      await useCollectionsStore.getState().refresh(collectionId);
    } catch (err) {
      reportError(err);
    } finally {
      if (!closed.current) setBusy(false);
    }
  };

  const showDiff = async (path: string) => {
    try {
      const text = await api.diff(collectionId, path);
      if (!closed.current) setSelected({ path, text });
    } catch (err) {
      reportError(err);
    }
  };

  const title = `Git — ${name ?? "Collection"}`;
  if (!status)
    return (
      <Dialog open title={title} onClose={onClose} cancelLabel="Close">
        <p>Loading…</p>
      </Dialog>
    );
  if (!status.git_available) {
    return (
      <Dialog open title={title} onClose={onClose} cancelLabel="Close">
        <p role="alert">Git could not be started. Install Git and try again.</p>
      </Dialog>
    );
  }
  if (!status.is_repo) {
    return (
      <Dialog
        open
        title={title}
        onClose={onClose}
        cancelLabel="Close"
        actions={
          <Button
            label="Initialise repository"
            disabled={busy}
            onClick={() => void act(() => api.init(collectionId))}
          />
        }
      >
        <p>
          This collection is not a Git repository yet. Initialising also adds a
          .gitignore for local-only files.
        </p>
      </Dialog>
    );
  }

  const conflicts = conflictsOf(status.files);
  const sync = syncSummary(status.ahead, status.behind);
  const canCommit =
    !busy &&
    conflicts.length === 0 &&
    message.trim() !== "" &&
    (status.files.length > 0 || status.merging);

  const resolve = (path: string, side: ConflictSide) =>
    void act(() => api.resolve(collectionId, path, side));

  return (
    <Dialog open title={title} onClose={onClose} cancelLabel="Close">
      <div className="git-bar">
        <label>
          Branch
          <select
            aria-label="Branch"
            value={status.branch ?? ""}
            disabled={busy}
            onChange={(e) =>
              void act(() => api.switchBranch(collectionId, e.target.value))
            }
          >
            {status.branch === null ? (
              <option value="">(detached)</option>
            ) : null}
            {branches.map((b) => (
              <option key={b.name} value={b.name}>
                {b.name}
              </option>
            ))}
          </select>
        </label>
        <span aria-label="Sync status">
          {sync || (status.upstream ? "Up to date" : "No upstream")}
        </span>
        <Button
          label="Pull"
          disabled={busy}
          onClick={() => void act(() => api.pull(collectionId))}
        />
        <Button
          label="Push"
          disabled={busy}
          onClick={() => void act(() => api.push(collectionId))}
        />
        <Button
          label="Refresh"
          disabled={busy}
          onClick={() => void refresh()}
        />
      </div>
      <div className="git-bar">
        <input
          aria-label="New branch name"
          value={newBranch}
          placeholder="new-branch"
          onChange={(e) => setNewBranch(e.target.value)}
        />
        <Button
          label="Create branch"
          disabled={busy || newBranch.trim() === ""}
          onClick={() =>
            void act(async () => {
              const s = await api.createBranch(collectionId, newBranch);
              setNewBranch("");
              return s;
            })
          }
        />
      </div>

      {conflicts.length > 0 ? (
        <section aria-label="Conflicts">
          <h4>Conflicts</h4>
          <ul>
            {conflicts.map((f) => (
              <li key={f.path}>
                {f.path}{" "}
                <Button
                  label="Take mine"
                  aria-label={`Take mine ${f.path}`}
                  disabled={busy}
                  onClick={() => resolve(f.path, "mine")}
                />
                <Button
                  label="Take theirs"
                  aria-label={`Take theirs ${f.path}`}
                  disabled={busy}
                  onClick={() => resolve(f.path, "theirs")}
                />
              </li>
            ))}
          </ul>
        </section>
      ) : null}

      <section aria-label="Changes">
        <h4>Changes ({status.files.length})</h4>
        {status.files.length === 0 ? (
          <p>
            {status.merging
              ? "Merge in progress — commit to finish."
              : "Nothing to commit."}
          </p>
        ) : null}
        <ul>
          {status.files.map((f) => (
            <li key={f.path}>
              <button
                type="button"
                className="link"
                onClick={() => void showDiff(f.path)}
              >
                {f.path}
              </button>{" "}
              <small>{changeLabel(f)}</small>
            </li>
          ))}
        </ul>
      </section>

      {selected ? (
        <pre aria-label={`Diff of ${selected.path}`} className="git-diff">
          {selected.text === ""
            ? "No textual changes."
            : selected.text.split("\n").map((line, i) => (
                <div key={i} className={diffLineClass(line)}>
                  {line}
                </div>
              ))}
        </pre>
      ) : null}

      <label>
        Commit message
        <textarea
          aria-label="Commit message"
          value={message}
          rows={2}
          onChange={(e) => setMessage(e.target.value)}
        />
      </label>
      <Button
        label="Commit"
        disabled={!canCommit}
        onClick={() => void act(() => api.commit(collectionId, message), true)}
      />
    </Dialog>
  );
}
