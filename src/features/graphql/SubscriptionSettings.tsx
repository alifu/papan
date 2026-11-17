import type { EffectiveVariable, SubscriptionSettings as Settings } from "../../ipc/bindings";
import { CodeEditor } from "../../ui/code/CodeEditor";
import { VariablesEditor } from "./VariablesEditor";

export interface SubscriptionSettingsProps {
  readonly settings: Settings;
  readonly variables?: readonly EffectiveVariable[];
  readonly onChange: (settings: Settings) => void;
}

/** The `subscription` section: a WebSocket URL override and the `connection_init` payload. */
export function SubscriptionSettings({ settings, variables, onChange }: SubscriptionSettingsProps) {
  return (
    <div className="editor-form">
      <label>
        WebSocket URL (optional; derived from the request URL when empty)
        <div className="url-field">
          <CodeEditor
            singleLine
            language="text"
            ariaLabel="WebSocket URL"
            value={settings.url}
            variables={variables}
            onChange={(url) => onChange({ ...settings, url })}
          />
        </div>
      </label>
      <p>Connection init payload (JSON object)</p>
      <VariablesEditor
        label="Connection init payload"
        value={settings.init_payload}
        variables={variables}
        onChange={(init_payload) => onChange({ ...settings, init_payload })}
      />
    </div>
  );
}
