import type { EffectiveVariable, RequestContent, RequestLocation } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { CodeEditor } from "../../ui/code/CodeEditor";
import { usePreviewTitle } from "../environments/usePreviewTitle";
import { useWsStore } from "../ws/wsStore";
import { statusText, subscriptionOf, wsUrlOf } from "./subscriptionModel";
import "../ws/ws.css";

export interface SubscriptionBarProps {
  readonly tabId: string;
  readonly location: RequestLocation;
  readonly content: RequestContent;
  readonly variables: readonly EffectiveVariable[] | undefined;
  readonly dirty: boolean;
  readonly onUrl: (url: string) => void;
  readonly onSave: () => void;
}

/** The URL bar of a subscription: Start / Stop instead of Send, a status pill and the derived ws(s) URL. */
export function SubscriptionBar({ tabId, location, content, variables, dirty, onUrl, onSave }: SubscriptionBarProps) {
  const tab = useWsStore((s) => s.byTab[tabId]);
  const actions = useWsStore.getState;
  const status = tab?.status ?? "idle";
  const preview = usePreviewTitle(location, content.url);
  const live = status === "connecting" || status === "open";
  const toggle = () => {
    if (status === "closing") return;
    if (live) void actions().unsubscribe(tabId);
    else void actions().subscribe(tabId, location, content);
  };

  return (
    <div
      className="subscription-bar"
      onKeyDown={(e) => {
        if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
          e.preventDefault();
          toggle();
        }
      }}
    >
      <div className="request-line">
        <div className="url-field" title={preview}>
          <CodeEditor singleLine language="text" ariaLabel="URL" value={content.url} variables={variables} onChange={onUrl} />
        </div>
        {content.settings.verify_tls ? null : (
          <span className="tls-badge" title="Certificate checks are off for this request">TLS not verified</span>
        )}
        <span className={`ws-status ws-status-${status}`} aria-label="Subscription status">
          {statusText(tab)}
        </span>
        <Button label={live ? "Stop" : "Start"} disabled={status === "closing"} onClick={toggle} />
        <Button label="Save" disabled={!dirty} onClick={onSave} />
      </div>
      <p className="ws-protocol" aria-label="Subscription endpoint">
        {wsUrlOf(content.url, subscriptionOf(content).url)}
      </p>
    </div>
  );
}
