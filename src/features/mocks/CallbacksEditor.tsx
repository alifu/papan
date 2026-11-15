import type { Callback } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { HeadersEditor } from "./HeadersEditor";
import {
  MAX_CALLBACKS,
  METHODS,
  newCallback,
  removeAt,
  replaceAt,
} from "./mockModel";

export interface CallbacksEditorProps {
  /** Names the controls, e.g. "Response 1". */
  readonly label: string;
  readonly callbacks: readonly Callback[];
  readonly onChange: (callbacks: Callback[]) => void;
}

const CALLBACK_METHODS = METHODS.filter((m) => m !== "any");

/** Requests sent after the response; the URL is fixed, the body can use templates. */
export function CallbacksEditor({
  label,
  callbacks,
  onChange,
}: CallbacksEditorProps) {
  const set = (index: number, patch: Partial<Callback>) =>
    onChange(replaceAt(callbacks, index, { ...callbacks[index]!, ...patch }));
  return (
    <div
      className="mock-callbacks"
      role="group"
      aria-label={`${label} callbacks`}
    >
      {callbacks.map((callback, index) => {
        const n = index + 1;
        return (
          <fieldset key={index} className="mock-callback">
            <legend>Callback {n}</legend>
            <div className="mock-row">
              <select
                aria-label={`${label} callback ${n} method`}
                value={callback.method}
                onChange={(e) => set(index, { method: e.target.value })}
              >
                {CALLBACK_METHODS.map((m) => (
                  <option key={m} value={m}>
                    {m.toUpperCase()}
                  </option>
                ))}
              </select>
              <input
                aria-label={`${label} callback ${n} URL`}
                value={callback.url}
                placeholder="https://hooks.example.com/notify"
                onChange={(e) => set(index, { url: e.target.value })}
              />
              <label>
                Delay (ms)
                <input
                  aria-label={`${label} callback ${n} delay`}
                  inputMode="numeric"
                  value={callback.delay_ms ?? 0}
                  onChange={(e) =>
                    set(index, {
                      delay_ms: Math.max(0, Number(e.target.value) || 0),
                    })
                  }
                />
              </label>
              <Button
                icon="close"
                aria-label={`Remove ${label} callback ${n}`}
                onClick={() => onChange(removeAt(callbacks, index))}
              />
            </div>
            <HeadersEditor
              label={`${label} callback ${n} header`}
              headers={callback.headers ?? []}
              onChange={(headers) => set(index, { headers })}
            />
            <textarea
              aria-label={`${label} callback ${n} body`}
              value={callback.body ?? ""}
              rows={3}
              spellCheck={false}
              placeholder="Body (templates work here, the URL is fixed)"
              onChange={(e) => set(index, { body: e.target.value })}
            />
          </fieldset>
        );
      })}
      <Button
        icon="plus"
        label="Add callback"
        aria-label={`Add ${label} callback`}
        disabled={callbacks.length >= MAX_CALLBACKS}
        onClick={() => onChange([...callbacks, newCallback()])}
      />
    </div>
  );
}
