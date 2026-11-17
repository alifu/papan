import type { Timing } from "../../ipc/bindings";

export const TIMING_NOTE =
  "Diagnostic only: sends running at once on the same client can pick up each other's timings. With a proxy, DNS is the proxy's lookup.";

const PHASES = [
  { key: "dns_ms", label: "DNS", color: "var(--timing-dns)" },
  { key: "connect_ms", label: "Connect + TLS", color: "var(--timing-connect)" },
  { key: "wait_ms", label: "Waiting", color: "var(--timing-wait)" },
  { key: "download_ms", label: "Download", color: "var(--timing-download)" },
] as const;

export function TimingBar({ timing }: { timing: Timing }) {
  const total = PHASES.reduce((sum, { key }) => sum + timing[key], 0);
  return (
    <div className="timing-bar">
      <div className="timing-track" role="img" aria-label={`Total ${total} ms`}>
        {PHASES.map(({ key, label, color }) =>
          timing[key] > 0 ? (
            <span
              key={key}
              className="timing-segment"
              data-phase={key}
              title={label}
              style={{ flexGrow: timing[key], background: color }}
            />
          ) : null,
        )}
      </div>
      <ul className="timing-legend">
        {PHASES.map(({ key, label, color }) => (
          <li key={key}>
            <span className="timing-swatch" style={{ background: color }} aria-hidden="true" />
            <span className="timing-label">{label}</span>
            <span className="timing-ms">{timing[key]} ms</span>
          </li>
        ))}
      </ul>
      {timing.reused ? <p className="timing-reused">Connection reused</p> : null}
      <p className="timing-note">{TIMING_NOTE}</p>
    </div>
  );
}
