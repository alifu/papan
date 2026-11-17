import { useEffect, useState } from "react";
import type { KeyValue, RequestLocation } from "../../ipc/bindings";
import { Tabs } from "../../ui/Tabs";
import { MessageLog } from "../ws/MessageLog";
import { statusTone } from "./grpcModel";
import { useGrpcStore } from "./grpcStore";
import "../ws/ws.css";
import "./grpc.css";

export interface CallPanelProps {
  readonly tabId: string;
  readonly location: RequestLocation;
}

const NO_ENTRIES: readonly KeyValue[] = [];

function Metadata({ label, entries }: { readonly label: string; readonly entries: readonly KeyValue[] | null }) {
  if (entries === null || entries.length === 0) return <p className="empty-text">No {label.toLowerCase()}.</p>;
  return (
    <dl className="grpc-metadata" aria-label={label}>
      {entries.map((e, i) => (
        <div key={`${e.name}-${i}`} className="grpc-metadata-row">
          <dt>{e.name}</dt>
          <dd>{e.value}</dd>
        </div>
      ))}
    </dl>
  );
}

/** Replaces the response pane for a gRPC request: status line, message log, response headers and trailers. */
export function CallPanel({ tabId, location }: CallPanelProps) {
  const tab = useGrpcStore((s) => s.byTab[tabId]);
  const [section, setSection] = useState("messages");
  const { restore, clear } = useGrpcStore.getState();
  useEffect(() => {
    void restore(tabId, location);
  }, [tabId]);

  const log = tab?.log ?? [];
  const status = tab?.grpcStatus ?? null;
  const tabs = [
    { id: "messages", label: "Messages", count: log.length },
    { id: "headers", label: "Headers" },
    { id: "trailers", label: "Trailers" },
  ];

  return (
    <div className="ws-panel">
      {tab?.error ? (
        <p role="alert" className="field-error">
          {tab.error.message}
        </p>
      ) : null}
      {status ? (
        <p className={`grpc-status-line grpc-tone-${statusTone(status)}`} aria-label="gRPC status">
          <strong>{status.name}</strong>
          {status.message === "" ? "" : `: ${status.message}`}
          {tab?.elapsedMs == null ? "" : ` · ${Math.round(tab.elapsedMs)} ms`}
        </p>
      ) : null}
      <Tabs tabs={tabs} selectedId={section} onSelect={setSection} label="Response sections">
        {section === "messages" ? <MessageLog messages={log} onClear={() => void clear(tabId)} /> : null}
        {section === "headers" ? <Metadata label="Headers" entries={tab?.headers ?? null} /> : null}
        {section === "trailers" ? <Metadata label="Trailers" entries={tab?.trailers ?? NO_ENTRIES} /> : null}
      </Tabs>
    </div>
  );
}
