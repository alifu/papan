import { useState } from "react";
import { Button } from "../../ui/Button";
import { subprotocolError } from "./wsModel";
import "./ws.css";

export interface SubprotocolsEditorProps {
  readonly value: readonly string[];
  readonly onChange: (value: readonly string[]) => void;
}

/** The `Sec-WebSocket-Protocol` offers, in order. */
export function SubprotocolsEditor({ value, onChange }: SubprotocolsEditorProps) {
  const [draft, setDraft] = useState("");
  const error = draft === "" ? null : subprotocolError(draft, value);
  const canAdd = draft !== "" && error === null;
  const add = () => {
    if (!canAdd) return;
    onChange([...value, draft]);
    setDraft("");
  };

  return (
    <div className="editor-form">
      <ul className="ws-subprotocols">
        {value.map((p) => (
          <li key={p} className="ws-subprotocol-row">
            <span>{p}</span>
            <Button label="Remove" aria-label={`Remove ${p}`} onClick={() => onChange(value.filter((x) => x !== p))} />
          </li>
        ))}
      </ul>
      <div className="ws-subprotocol-row">
        <input
          aria-label="New subprotocol"
          aria-invalid={error !== null}
          placeholder="e.g. graphql-transport-ws"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") add();
          }}
        />
        <Button label="Add" disabled={!canAdd} onClick={add} />
      </div>
      {error ? <p role="alert" className="field-error">{error}</p> : null}
    </div>
  );
}
