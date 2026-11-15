import { useEffect, useRef, useState } from "react";
import type { WsDirection, WsMessage } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { expandedText, formatTime, previewOf } from "./wsModel";
import "./ws.css";

export interface MessageLogProps {
  readonly messages: readonly WsMessage[];
  readonly onClear: () => void;
}

type DirectionFilter = WsDirection | "all";

const NEAR_BOTTOM_PX = 24;
const ARROW: Record<WsDirection, string> = { sent: "↑", received: "↓" };
const LABEL: Record<WsDirection, string> = { sent: "Sent", received: "Received" };

const matches = (m: WsMessage, direction: DirectionFilter, text: string): boolean =>
  (direction === "all" || m.direction === direction) && (text === "" || m.text.toLowerCase().includes(text.toLowerCase()));

/** The frame log: filterable rows that expand to the full body; auto-scroll pauses while scrolled up. */
export function MessageLog({ messages, onClear }: MessageLogProps) {
  const [direction, setDirection] = useState<DirectionFilter>("all");
  const [text, setText] = useState("");
  const [open, setOpen] = useState<ReadonlySet<number>>(new Set());
  const list = useRef<HTMLUListElement>(null);
  const stuck = useRef(true);
  const shown = messages.filter((m) => matches(m, direction, text));

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
        <select aria-label="Direction" value={direction} onChange={(e) => setDirection(e.target.value as DirectionFilter)}>
          <option value="all">All</option>
          <option value="sent">Sent</option>
          <option value="received">Received</option>
        </select>
        <input aria-label="Filter text" placeholder="Filter" value={text} onChange={(e) => setText(e.target.value)} />
        <Button label="Clear" disabled={messages.length === 0} onClick={onClear} />
      </div>
      {messages.length === 0 ? <p className="empty-text">No messages yet.</p> : null}
      {messages.length > 0 && shown.length === 0 ? <p className="empty-text">No messages match the filter.</p> : null}
      <ul ref={list} className="ws-log-list" aria-label="Message log" onScroll={onScroll}>
        {shown.map((m) => {
          const expanded = open.has(m.seq);
          return (
            <li key={m.seq} className={`ws-row ws-${m.direction}`}>
              <button
                type="button"
                className="ws-row-head"
                aria-expanded={expanded}
                aria-label={`${expanded ? "Collapse" : "Expand"} message ${m.seq}`}
                onClick={() => toggle(m.seq)}
              >
                <span className="ws-arrow" role="img" aria-label={LABEL[m.direction]}>
                  {ARROW[m.direction]}
                </span>
                <span className="ws-time">{formatTime(m.at_ms)}</span>
                <span className="ws-frame">{m.frame}</span>
                <span className="ws-size">{m.size} B</span>
                <span className="ws-preview">{previewOf(m)}</span>
                {m.truncated ? <span className="ws-truncated">truncated</span> : null}
              </button>
              {expanded ? (
                <pre className="ws-body" aria-label={`Message ${m.seq} body`}>
                  {expandedText(m)}
                </pre>
              ) : null}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
