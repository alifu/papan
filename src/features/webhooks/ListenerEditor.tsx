import { useState, type FormEvent } from "react";
import type { WebhookBind, WebhookListener } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { ResponseEditor } from "./ResponseEditor";
import { formFromConfig, validateForm, type ListenerForm } from "./schema";
import { useWebhookStore } from "./webhookStore";

export const LAN_WARNING =
  "All interfaces: anyone on your network can send requests to this listener. Prefer loopback unless you need a device on your LAN.";

export function ListenerEditor({ listener }: { readonly listener: WebhookListener }) {
  const id = listener.config.id;
  const [form, setForm] = useState<ListenerForm>(() => formFromConfig(listener.config));
  const [errors, setErrors] = useState<readonly string[]>([]);
  const patch = (next: Partial<ListenerForm>) => setForm((current) => ({ ...current, ...next }));

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const result = validateForm(id, form);
    if (!result.ok) {
      setErrors(result.errors);
      return;
    }
    setErrors([]);
    await useWebhookStore.getState().save(result.config);
  };

  return (
    <form className="listener-form" aria-label="Listener settings" onSubmit={(e) => void submit(e)}>
      <label>
        Name
        <input value={form.name} onChange={(e) => patch({ name: e.target.value })} />
      </label>
      <label>
        Bind
        <select value={form.bind} onChange={(e) => patch({ bind: e.target.value as WebhookBind })}>
          <option value="loopback">Loopback only (127.0.0.1)</option>
          <option value="all_interfaces">All interfaces (LAN)</option>
        </select>
      </label>
      {form.bind === "all_interfaces" ? (
        <p className="webhook-warning" role="note">
          {LAN_WARNING}
        </p>
      ) : null}
      <label>
        Port (0 for any free port)
        <input value={form.port} inputMode="numeric" onChange={(e) => patch({ port: e.target.value })} />
      </label>
      <label>
        Path
        <input value={form.path} onChange={(e) => patch({ path: e.target.value })} />
      </label>
      <label>
        Body limit (bytes)
        <input
          value={form.maxBodyBytes}
          inputMode="numeric"
          onChange={(e) => patch({ maxBodyBytes: e.target.value })}
        />
      </label>
      <label>
        Captures to keep
        <input
          value={form.maxCaptures}
          inputMode="numeric"
          onChange={(e) => patch({ maxCaptures: e.target.value })}
        />
      </label>
      <ResponseEditor form={form} onChange={patch} />
      {errors.length > 0 ? (
        <ul role="alert" className="listener-errors">
          {errors.map((message) => (
            <li key={message}>{message}</li>
          ))}
        </ul>
      ) : null}
      <div>
        <Button type="submit" label={listener.running ? "Save and restart" : "Save"} />
      </div>
    </form>
  );
}
