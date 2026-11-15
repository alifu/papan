import type { EffectiveVariable, RequestContent, RequestLocation } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { CodeEditor } from "../../ui/code/CodeEditor";
import { usePreviewTitle } from "../environments/usePreviewTitle";
import { useWsStore, type WsStatus } from "./wsStore";
import "./ws.css";

export interface ConnectBarProps {
  readonly tabId: string;
  readonly location: RequestLocation;
  readonly content: RequestContent;
  readonly variables: readonly EffectiveVariable[] | undefined;
  readonly dirty: boolean;
  readonly onUrl: (url: string) => void;
  readonly onSave: () => void;
}

const STATUS_LABEL: Record<WsStatus, string> = {
  idle: "Idle",
  connecting: "Connecting",
  open: "Open",
  closing: "Closing",
  closed: "Closed",
};

/** The WebSocket URL bar: Connect / Disconnect instead of Send, a status pill and the negotiated subprotocol. */
export function ConnectBar({ tabId, location, content, variables, dirty, onUrl, onSave }: ConnectBarProps) {
  const tab = useWsStore((s) => s.byTab[tabId]);
  const actions = useWsStore.getState;
  const status = tab?.status ?? "idle";
  const preview = usePreviewTitle(location, content.url);
  const busy = status === "connecting" || status === "open";
  const connect = () => {
    if (!busy && status !== "closing") void actions().connect(tabId, location, content);
  };

  return (
    <div
      className="request-line"
      onKeyDown={(e) => {
        if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
          e.preventDefault();
          connect();
        }
      }}
    >
      <div className="url-field" title={preview}>
        <CodeEditor singleLine language="text" ariaLabel="URL" value={content.url} variables={variables} onChange={onUrl} />
      </div>
      {content.settings.verify_tls ? null : (
        <span className="tls-badge" title="Certificate checks are off for this request">TLS not verified</span>
      )}
      <span className={`ws-status ws-status-${status}`} aria-label="Connection status">
        {STATUS_LABEL[status]}
      </span>
      {tab?.opened?.protocol ? <span className="ws-protocol" title="Negotiated subprotocol">{tab.opened.protocol}</span> : null}
      {status === "connecting" ? (
        <Button label="Cancel" onClick={() => void actions().disconnect(tabId)} />
      ) : busy || status === "closing" ? (
        <Button label="Disconnect" disabled={status === "closing"} onClick={() => void actions().disconnect(tabId)} />
      ) : (
        <Button label="Connect" onClick={connect} />
      )}
      <Button label="Save" disabled={!dirty} onClick={onSave} />
    </div>
  );
}
