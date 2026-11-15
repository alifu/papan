import { Button } from "../../ui/Button";
import { MAX_RESPONSE_HEADERS, type ListenerForm } from "./schema";

export interface ResponseEditorProps {
  readonly form: ListenerForm;
  readonly onChange: (patch: Partial<ListenerForm>) => void;
}

type HeaderRow = ListenerForm["headers"][number];

export function ResponseEditor({ form, onChange }: ResponseEditorProps) {
  const setHeader = (index: number, patch: Partial<HeaderRow>) =>
    onChange({ headers: form.headers.map((row, i) => (i === index ? { ...row, ...patch } : row)) });

  return (
    <fieldset className="listener-form">
      <legend>Response</legend>
      <label>
        Status
        <input value={form.status} inputMode="numeric" onChange={(e) => onChange({ status: e.target.value })} />
      </label>
      <div className="listener-form-headers" role="group" aria-label="Response headers">
        {form.headers.map((row, index) => (
          <div key={index}>
            <input
              aria-label={`Header name ${index + 1}`}
              value={row.name}
              onChange={(e) => setHeader(index, { name: e.target.value })}
            />
            <input
              aria-label={`Header value ${index + 1}`}
              value={row.value}
              onChange={(e) => setHeader(index, { value: e.target.value })}
            />
            <Button
              icon="close"
              aria-label={`Remove header ${index + 1}`}
              onClick={() => onChange({ headers: form.headers.filter((_, i) => i !== index) })}
            />
          </div>
        ))}
        <Button
          icon="plus"
          label="Add header"
          disabled={form.headers.length >= MAX_RESPONSE_HEADERS}
          onClick={() => onChange({ headers: [...form.headers, { name: "", value: "" }] })}
        />
      </div>
      <label>
        Response body
        <textarea value={form.body} onChange={(e) => onChange({ body: e.target.value })} />
      </label>
    </fieldset>
  );
}
