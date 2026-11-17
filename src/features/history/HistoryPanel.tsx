import { useEffect } from "react";
import type { HistoryEntry } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { useTabsStore } from "../../shell/tabsStore";
import { kindBadge } from "../collections/requestKind";
import { scratchLocation } from "../locations";
import { useHistoryStore } from "./historyStore";
import { relativeTime } from "./relativeTime";
import "../response/viewer.css";

const outcome = (entry: HistoryEntry): string =>
  entry.status !== null ? String(entry.status) : (entry.error_code ?? "error");

/** History list: click restores the entry as a scratch request and opens it. */
export function HistoryPanel() {
  const entries = useHistoryStore((s) => s.entries);
  const load = useHistoryStore((s) => s.load);
  const clear = useHistoryStore((s) => s.clear);
  const restore = useHistoryStore((s) => s.restore);
  const openRequest = useTabsStore((s) => s.openRequest);

  useEffect(() => {
    void load();
  }, [load]);

  const open = async (id: string) => {
    const item = await restore(id);
    if (item) openRequest(scratchLocation(item.id), item.name);
  };

  const now = Date.now();
  return (
    <div className="history-panel">
      <Button label="Clear" disabled={entries.length === 0} onClick={() => void clear()} />
      {entries.length === 0 ? (
        <p className="response-notice">No requests sent yet.</p>
      ) : (
        <ul className="history-list">
          {entries.map((entry) => (
            <li key={entry.id}>
              <button type="button" className="history-item" onClick={() => void open(entry.id)}>
                <span>{kindBadge(entry.kind, entry.method)}</span>
                <span className="history-url" title={entry.url}>{entry.url}</span>
                <span className={entry.status === null ? "history-error" : undefined}>{outcome(entry)}</span>
                <span>{relativeTime(entry.sent_at_ms, now)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
