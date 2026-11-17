import { useEffect, useMemo, useState } from "react";
import type { RequestLocation } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { MessageLog } from "../ws/MessageLog";
import { useWsStore } from "../ws/wsStore";
import { ResultLog } from "./ResultLog";
import { resultsOf } from "./subscriptionModel";
import "../ws/ws.css";

export interface SubscriptionPanelProps {
  readonly tabId: string;
  readonly location: RequestLocation;
}

/** Replaces the response pane for a subscription: the result log, or the raw protocol frames. */
export function SubscriptionPanel({ tabId, location }: SubscriptionPanelProps) {
  const tab = useWsStore((s) => s.byTab[tabId]);
  const [frames, setFrames] = useState(false);
  const log = tab?.log;
  const results = useMemo(() => resultsOf(log ?? []), [log]);
  const { restore, clear } = useWsStore.getState();
  useEffect(() => {
    void restore(tabId, location);
  }, [tabId]);

  return (
    <div className="ws-panel">
      {tab?.error ? (
        <p role="alert" className="field-error">
          {tab.error.message}
        </p>
      ) : null}
      <div className="ws-log-bar">
        <Button label="Protocol frames" pressed={frames} onClick={() => setFrames((on) => !on)} />
      </div>
      {frames ? (
        <MessageLog messages={log ?? []} onClear={() => void clear(tabId)} />
      ) : (
        <ResultLog results={results} onClear={() => void clear(tabId)} />
      )}
    </div>
  );
}
