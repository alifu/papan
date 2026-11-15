import { useState } from "react";
import type { MockLogEntry } from "../../ipc/bindings";
import { Button } from "../../ui/Button";

export interface MockLogProps {
  readonly entries: readonly MockLogEntry[];
  readonly onClear: () => void;
}

const time = (ms: number): string => new Date(ms).toLocaleTimeString();

export function MockLog({ entries, onClear }: MockLogProps) {
  const [openId, setOpenId] = useState<number | null>(null);
  const open = entries.find((e) => e.id === openId);
  return (
    <div className="mock-log">
      <div className="mock-row">
        <h2>Request log ({entries.length})</h2>
        <Button
          label="Clear log"
          disabled={entries.length === 0}
          onClick={onClear}
        />
      </div>
      {entries.length === 0 ? (
        <p className="empty-text">No requests yet.</p>
      ) : null}
      <ul aria-label="Requests" className="mock-log-list">
        {[...entries].reverse().map((entry) => (
          <li key={entry.id}>
            <button
              type="button"
              className="link"
              onClick={() => setOpenId(entry.id === openId ? null : entry.id)}
            >
              <span>{time(entry.at_ms ?? 0)}</span> <b>{entry.method}</b>{" "}
              {entry.path}
              {entry.query ? `?${entry.query}` : ""} → {entry.status}{" "}
              <small>
                {entry.route_id ? "" : "(no route)"} {entry.duration_ms} ms
              </small>
            </button>
          </li>
        ))}
      </ul>
      {open ? (
        <section aria-label="Request detail" className="mock-log-detail">
          <h3>
            {open.method} {open.path}
          </h3>
          <dl>
            {open.headers.map((h) => (
              <div key={h.name}>
                <dt>{h.name}</dt>
                <dd>{h.value}</dd>
              </div>
            ))}
          </dl>
          {open.body_text ? (
            <pre>
              {open.body_text}
              {open.body_truncated ? "\n… truncated" : ""}
            </pre>
          ) : null}
        </section>
      ) : null}
    </div>
  );
}
