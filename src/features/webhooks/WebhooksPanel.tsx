import { useTabsStore } from "../../shell/tabsStore";
import { Button } from "../../ui/Button";
import { Menu } from "../../ui/Menu";
import { MAX_LISTENERS } from "./schema";
import { useWebhookStore } from "./webhookStore";
import "./webhooks.css";

const EMPTY_TEXT = "No webhook listeners yet.";

export function WebhooksPanel() {
  const listeners = useWebhookStore((s) => s.listeners);

  const create = async () => {
    const listener = await useWebhookStore.getState().create();
    if (listener) useTabsStore.getState().openWebhook(listener.config.id, listener.config.name);
  };

  return (
    <div className="webhooks-panel">
      <div className="panel-actions">
        <Button
          icon="plus"
          label="New Listener"
          disabled={listeners.length >= MAX_LISTENERS}
          onClick={() => void create()}
        />
      </div>
      {listeners.length === 0 ? (
        <p className="empty-text">{EMPTY_TEXT}</p>
      ) : (
        <ul className="scratch-list" aria-label="Webhook listeners">
          {listeners.map(({ config, running, captured }) => (
            <li key={config.id} className="tree-row">
              <span
                className={running ? "webhook-dot webhook-dot-on" : "webhook-dot"}
                role="img"
                aria-label={running ? "Running" : "Stopped"}
              />
              <button
                type="button"
                className="scratch-open"
                onClick={() => useTabsStore.getState().openWebhook(config.id, config.name)}
              >
                <span className="tree-label">{config.name}</span>
                <span className="tree-method">{captured}</span>
              </button>
              <Button
                icon={running ? "stop" : "play"}
                className="tree-menu"
                aria-label={`${running ? "Stop" : "Start"} ${config.name}`}
                onClick={() => {
                  const store = useWebhookStore.getState();
                  void (running ? store.stop(config.id) : store.start(config.id));
                }}
              />
              <Menu
                label={`Actions for ${config.name}`}
                icon="chevron-down"
                className="tree-menu"
                items={[{ label: "Delete", onSelect: () => void useWebhookStore.getState().remove(config.id) }]}
              />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
