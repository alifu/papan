import { Button } from "../../ui/Button";
import { CallbacksEditor } from "./CallbacksEditor";
import { HeadersEditor } from "./HeadersEditor";
import { newFallback, type MResponse } from "./mockModel";

export interface FallbackEditorProps {
  readonly fallback: MResponse | null;
  readonly onChange: (fallback: MResponse | null) => void;
}

/** The answer for requests that match no route (instead of the built-in 404). */
export function FallbackEditor({ fallback, onChange }: FallbackEditorProps) {
  if (!fallback) {
    return (
      <div className="mock-fallback">
        <Button
          label="Add fallback response"
          onClick={() => onChange(newFallback())}
        />
      </div>
    );
  }
  const set = (patch: Partial<MResponse>) =>
    onChange({ ...fallback, ...patch });
  return (
    <fieldset className="mock-fallback">
      <legend>Fallback response (no route matched)</legend>
      <div className="mock-row">
        <label>
          Status
          <input
            aria-label="Fallback status"
            inputMode="numeric"
            value={fallback.status}
            onChange={(e) => set({ status: Number(e.target.value) || 0 })}
          />
        </label>
        <label>
          Latency (ms)
          <input
            aria-label="Fallback latency"
            inputMode="numeric"
            value={fallback.latency_ms}
            onChange={(e) =>
              set({ latency_ms: Math.max(0, Number(e.target.value) || 0) })
            }
          />
        </label>
        <Button label="Remove fallback" onClick={() => onChange(null)} />
      </div>
      <HeadersEditor
        label="Fallback header"
        headers={fallback.headers}
        onChange={(headers) => set({ headers })}
      />
      <label>
        File (path inside the collection; sent instead of the body)
        <input
          aria-label="Fallback file"
          value={fallback.file}
          onChange={(e) => set({ file: e.target.value })}
        />
      </label>
      <label>
        Body
        <textarea
          aria-label="Fallback body"
          value={fallback.body}
          rows={4}
          spellCheck={false}
          onChange={(e) => set({ body: e.target.value })}
        />
      </label>
      <CallbacksEditor
        label="Fallback"
        callbacks={fallback.callbacks}
        onChange={(callbacks) => set({ callbacks })}
      />
    </fieldset>
  );
}
