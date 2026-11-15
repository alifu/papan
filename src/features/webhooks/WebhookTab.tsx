import { useEffect, useState } from "react";
import { Button } from "../../ui/Button";
import { Tabs } from "../../ui/Tabs";
import { reportError, showToast } from "../../ui/toastStore";
import { copyText } from "../response/clipboard";
import { CaptureDetail } from "./CaptureDetail";
import { CaptureList } from "./CaptureList";
import { ListenerEditor } from "./ListenerEditor";
import { useWebhookStore } from "./webhookStore";
import "./webhooks.css";

const NO_CAPTURES: readonly never[] = [];

function tunnelHint(url: string): string {
  const port = new URL(url).port;
  return `To expose it publicly, run cloudflared tunnel --url http://127.0.0.1:${port} or ngrok http ${port}`;
}

export function WebhookTab({ webhookId }: { readonly webhookId: string }) {
  const listener = useWebhookStore((s) => s.listeners.find((l) => l.config.id === webhookId));
  const captures = useWebhookStore((s) => s.captures[webhookId] ?? NO_CAPTURES);
  const [view, setView] = useState("captures");
  const [selectedId, setSelectedId] = useState<string | null>(null);

  useEffect(() => {
    void useWebhookStore.getState().loadCaptures(webhookId);
  }, [webhookId]);

  if (!listener) return <p className="empty-text">This listener no longer exists.</p>;
  const { config, running, url } = listener;
  const store = useWebhookStore.getState();

  const copyUrl = async () => {
    if (!url) return;
    try {
      await copyText(url);
      showToast({ kind: "info", message: "URL copied." });
    } catch (err) {
      reportError(err);
    }
  };

  return (
    <div className="webhook-tab">
      <div className="webhook-head">
        <h1>{config.name}</h1>
        <span>{running ? "Running" : "Stopped"}</span>
        <Button
          icon={running ? "stop" : "play"}
          label={running ? "Stop" : "Start"}
          onClick={() => void (running ? store.stop(webhookId) : store.start(webhookId))}
        />
        {url ? (
          <>
            <code className="webhook-url">{url}</code>
            <Button label="Copy URL" onClick={() => void copyUrl()} />
          </>
        ) : null}
      </div>
      {url ? <p className="webhook-hint">{tunnelHint(url)}</p> : null}
      <Tabs
        label="Listener sections"
        selectedId={view}
        onSelect={setView}
        tabs={[
          { id: "captures", label: "Captures", count: captures.length },
          { id: "settings", label: "Settings" },
        ]}
      >
        {view === "captures" ? (
          <>
            <div className="webhook-head">
              <Button label="Clear captures" onClick={() => void store.clearCaptures(webhookId)} />
              {listener.unmatched > 0 ? <span>{listener.unmatched} unmatched</span> : null}
            </div>
            <div className="webhook-split">
              <CaptureList captures={captures} selectedId={selectedId} onSelect={setSelectedId} />
              {selectedId ? <CaptureDetail listener={listener} captureId={selectedId} /> : null}
            </div>
          </>
        ) : (
          <ListenerEditor key={JSON.stringify(config)} listener={listener} />
        )}
      </Tabs>
    </div>
  );
}
