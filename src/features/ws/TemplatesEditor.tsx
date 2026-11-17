import type { WsTemplate } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { templateNameError } from "./wsModel";
import "./ws.css";

export interface TemplatesEditorProps {
  readonly templates: readonly WsTemplate[];
  readonly onChange: (templates: readonly WsTemplate[]) => void;
  readonly onLoad: (template: WsTemplate) => void;
}

const replaceAt = (list: readonly WsTemplate[], index: number, next: WsTemplate): WsTemplate[] =>
  list.map((t, i) => (i === index ? next : t));

function moved(list: readonly WsTemplate[], from: number, to: number): WsTemplate[] {
  const next = [...list];
  const [item] = next.splice(from, 1);
  if (item) next.splice(to, 0, item);
  return next;
}

/** The saved messages (`ws.messages`) of the request. */
export function TemplatesEditor({ templates, onChange, onLoad }: TemplatesEditorProps) {
  if (templates.length === 0) {
    return <p className="empty-text">No saved messages. Write one in the composer and use Save as template.</p>;
  }
  return (
    <ul className="ws-templates">
      {templates.map((t, i) => {
        const others = templates.filter((_, j) => j !== i).map((o) => o.name);
        const error = templateNameError(t.name, others);
        return (
          <li key={i} className="ws-template-row">
            <input
              className="ws-template-name"
              aria-label={`Name of ${t.name}`}
              aria-invalid={error !== null}
              value={t.name}
              onChange={(e) => onChange(replaceAt(templates, i, { ...t, name: e.target.value }))}
            />
            <span className="ws-frame">{t.format}</span>
            <Button label="Load" aria-label={`Load ${t.name} into composer`} onClick={() => onLoad(t)} />
            <Button label="Up" aria-label={`Move ${t.name} up`} disabled={i === 0} onClick={() => onChange(moved(templates, i, i - 1))} />
            <Button
              label="Down"
              aria-label={`Move ${t.name} down`}
              disabled={i === templates.length - 1}
              onClick={() => onChange(moved(templates, i, i + 1))}
            />
            <Button label="Delete" aria-label={`Delete ${t.name}`} onClick={() => onChange(templates.filter((_, j) => j !== i))} />
            {error ? <span role="alert" className="field-error">{error}</span> : null}
          </li>
        );
      })}
    </ul>
  );
}
