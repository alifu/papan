import { useEffect, useRef, useState } from "react";
import { Button } from "../../ui/Button";
import { matchesFilter, type SubResult } from "./subscriptionModel";
import "../ws/ws.css";

export interface ResultLogProps {
  readonly results: readonly SubResult[];
  readonly onClear: () => void;
}

const NEAR_BOTTOM_PX = 24;

/** Subscription results: rows that expand to pretty JSON; filterable; auto-scroll pauses while scrolled up. */
export function ResultLog({ results, onClear }: ResultLogProps) {
  const [text, setText] = useState("");
  const [open, setOpen] = useState<ReadonlySet<number>>(new Set());
  const list = useRef<HTMLUListElement>(null);
  const stuck = useRef(true);
  const shown = results.filter((r) => matchesFilter(r, text));

  useEffect(() => {
    const el = list.current;
    if (el && stuck.current) el.scrollTop = el.scrollHeight;
  }, [shown.length]);

  const onScroll = () => {
    const el = list.current;
    if (el) stuck.current = el.scrollHeight - el.scrollTop - el.clientHeight <= NEAR_BOTTOM_PX;
  };
  const toggle = (seq: number) =>
    setOpen((prev) => {
      const next = new Set(prev);
      if (!next.delete(seq)) next.add(seq);
      return next;
    });

  return (
    <div className="ws-log">
      <div className="ws-log-bar">
        <input aria-label="Filter text" placeholder="Filter" value={text} onChange={(e) => setText(e.target.value)} />
        <span role="status">{results.length === 1 ? "1 result" : `${results.length} results`}</span>
        <Button label="Clear" disabled={results.length === 0} onClick={onClear} />
      </div>
      {results.length === 0 ? <p className="empty-text">No results yet.</p> : null}
      {results.length > 0 && shown.length === 0 ? <p className="empty-text">No results match the filter.</p> : null}
      <ul ref={list} className="ws-log-list" aria-label="Results" onScroll={onScroll}>
        {shown.map((r) => {
          const expanded = open.has(r.seq);
          return (
            <li key={r.seq} className={`ws-row ws-${r.kind === "error" ? "sent" : "received"}`}>
              <button
                type="button"
                className="ws-row-head"
                aria-expanded={expanded}
                aria-label={`${expanded ? "Collapse" : "Expand"} result ${r.seq}`}
                onClick={() => toggle(r.seq)}
              >
                <span className="ws-time">{r.time}</span>
                <span className="ws-frame">{r.kind}</span>
                <span className="ws-size">{r.size} B</span>
                <span className="ws-preview">{r.preview}</span>
              </button>
              {expanded && r.text !== "" ? (
                <pre className="ws-body" aria-label={`Result ${r.seq} body`}>
                  {r.text}
                </pre>
              ) : null}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
