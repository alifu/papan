import { useEffect } from "react";
import type { EffectiveVariable, RequestContent, RequestLocation } from "../../ipc/bindings";
import { MessageComposer } from "./MessageComposer";
import { MessageLog } from "./MessageLog";
import { upsertTemplate, wsSettingsOf } from "./wsModel";
import { useWsStore, type WsClosedInfo } from "./wsStore";
import "./ws.css";

export interface WsPanelProps {
  readonly tabId: string;
  readonly location: RequestLocation;
  readonly content: RequestContent;
  readonly variables: readonly EffectiveVariable[] | undefined;
  /** Receives a pure update; the owner applies it to its current content. */
  readonly onChange: (update: (content: RequestContent) => RequestContent) => void;
}

const BY_TEXT = { client: "Closed by you", server: "Closed by the server", error: "Connection lost" } as const;

function closedText(info: WsClosedInfo): string {
  const code = info.code === null ? "" : ` (${info.code})`;
  const reason = info.reason === "" ? "" : `: ${info.reason}`;
  return `${BY_TEXT[info.by]}${code}${reason}`;
}

/** Replaces the response pane for a WebSocket request: composer on top, message log below. */
export function WsPanel({ tabId, location, content, variables, onChange }: WsPanelProps) {
  const tab = useWsStore((s) => s.byTab[tabId]);
  const { restore, setComposer, send, clear } = useWsStore.getState();
  useEffect(() => {
    void restore(tabId, location);
  }, [tabId]);

  const composer = tab?.composer ?? { format: "text" as const, text: "" };
  const templates = wsSettingsOf(content).messages;

  return (
    <div className="ws-panel">
      {tab?.error ? <p role="alert" className="field-error">{tab.error.message}</p> : null}
      {tab?.closed ? <p className="ws-closed">{closedText(tab.closed)}</p> : null}
      <MessageComposer
        format={composer.format}
        text={composer.text}
        open={tab?.status === "open"}
        variables={variables}
        templateNames={templates.map((t) => t.name)}
        onFormat={(format) => setComposer(tabId, { format })}
        onText={(text) => setComposer(tabId, { text })}
        onSend={(message) => void send(tabId, location, message)}
        onSaveTemplate={(name) =>
          onChange((c) => {
            const ws = wsSettingsOf(c);
            const template = { name, format: composer.format, text: composer.text };
            return { ...c, ws: { ...ws, messages: upsertTemplate(ws.messages, template) } };
          })
        }
      />
      <MessageLog messages={tab?.log ?? []} onClear={() => void clear(tabId)} />
    </div>
  );
}
