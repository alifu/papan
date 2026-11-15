import type { CaptureSummary } from "../../ipc/bindings";

const pad = (n: number): string => String(n).padStart(2, "0");

export function formatTime(atMs: number | null): string {
  if (atMs === null) return "";
  const d = new Date(atMs);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

export interface CaptureListProps {
  readonly captures: readonly CaptureSummary[];
  readonly selectedId: string | null;
  readonly onSelect: (id: string) => void;
}

export function CaptureList({ captures, selectedId, onSelect }: CaptureListProps) {
  if (captures.length === 0) return <p className="empty-text">No requests captured yet.</p>;
  return (
    <ul className="capture-list" aria-label="Captured requests">
      {captures.map((c) => (
        <li key={c.id}>
          <button
            type="button"
            className="capture-row"
            aria-current={c.id === selectedId}
            onClick={() => onSelect(c.id)}
          >
            <strong>{c.method}</strong>
            <span className="capture-path">{c.path}</span>
            <span>{formatTime(c.at_ms)}</span>
            <span>{c.size} B</span>
          </button>
        </li>
      ))}
    </ul>
  );
}
