import { useEffect, useState } from "react";
import type { Capture, WebhookListener } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { reportError, showToast } from "../../ui/toastStore";
import { copyText } from "../response/clipboard";
import { captureToCurl } from "./curl";
import { useWebhookStore } from "./webhookStore";

function prettyBody(capture: Capture): string {
  if (capture.truncated) return "Body exceeded the limit and was not stored.";
  if (capture.body_text !== null) {
    try {
      return JSON.stringify(JSON.parse(capture.body_text), null, 2);
    } catch {
      return capture.body_text;
    }
  }
  return capture.body_base64 ?? "";
}

export interface CaptureDetailProps {
  readonly listener: WebhookListener;
  readonly captureId: string;
}

export function CaptureDetail({ listener, captureId }: CaptureDetailProps) {
  const id = listener.config.id;
  const [capture, setCapture] = useState<Capture | null>(null);

  useEffect(() => {
    let current = true;
    void useWebhookStore
      .getState()
      .readCapture(id, captureId)
      .then((next) => {
        if (current) setCapture(next);
      });
    return () => {
      current = false;
    };
  }, [id, captureId]);

  if (!capture) return <div className="capture-detail" aria-busy="true" />;

  const copyCurl = async () => {
    try {
      await copyText(captureToCurl(capture, listener.url ?? `http://127.0.0.1${listener.config.path}`));
      showToast({ kind: "info", message: "Copied as cURL." });
    } catch (err) {
      reportError(err);
    }
  };

  return (
    <section className="capture-detail" aria-label="Capture detail">
      <div className="webhook-head">
        <strong>
          {capture.summary.method} {capture.summary.path}
        </strong>
        <span>from {capture.remote}</span>
        <Button label="Replay as new request" onClick={() => void useWebhookStore.getState().replay(id, captureId)} />
        <Button label="Copy as cURL" onClick={() => void copyCurl()} />
      </div>
      {capture.query !== "" ? <p>Query: {capture.query}</p> : null}
      <dl className="capture-headers" aria-label="Headers">
        {capture.headers.map((h, index) => (
          <div key={index} style={{ display: "contents" }}>
            <dt>{h.name}</dt>
            <dd>{h.value}</dd>
          </div>
        ))}
      </dl>
      <pre aria-label="Body">{prettyBody(capture)}</pre>
    </section>
  );
}
