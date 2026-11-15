import { useState } from "react";
import type { EffectiveVariable, WsFormat, WsOutgoing } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { CodeEditor } from "../../ui/code/CodeEditor";
import type { EditorLanguage } from "../../ui/code/types";
import { base64Error, jsonWarning, templateNameError } from "./wsModel";
import "./ws.css";

export interface MessageComposerProps {
  readonly format: WsFormat;
  readonly text: string;
  readonly open: boolean;
  readonly variables?: readonly EffectiveVariable[];
  readonly templateNames: readonly string[];
  readonly onFormat: (format: WsFormat) => void;
  readonly onText: (text: string) => void;
  readonly onSend: (message: WsOutgoing) => void;
  /** Adds a template, or replaces the one with that name. */
  readonly onSaveTemplate: (name: string) => void;
}

const FORMATS: readonly { readonly value: WsFormat; readonly label: string }[] = [
  { value: "text", label: "Text" },
  { value: "json", label: "JSON" },
  { value: "binary", label: "Binary (base64)" },
];
const LANGUAGE: Record<WsFormat, EditorLanguage> = { text: "text", json: "json", binary: "text" };

export function MessageComposer(props: MessageComposerProps) {
  const { format, text, open, templateNames } = props;
  const [name, setName] = useState("");
  const warning = format === "json" ? jsonWarning(text) : null;
  const blocking = format === "binary" ? base64Error(text) : null;
  const canSend = open && blocking === null;
  const replacing = templateNames.includes(name);
  const nameError = replacing ? null : templateNameError(name, templateNames);
  const send = () => {
    if (canSend) props.onSend({ frame: format, text });
  };

  return (
    <div
      className="ws-composer"
      onKeyDown={(e) => {
        if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
          e.preventDefault();
          send();
        }
      }}
    >
      <div className="ws-composer-bar">
        <select aria-label="Format" value={format} onChange={(e) => props.onFormat(e.target.value as WsFormat)}>
          {FORMATS.map((f) => (
            <option key={f.value} value={f.value}>
              {f.label}
            </option>
          ))}
        </select>
        <Button label="Send" disabled={!canSend} onClick={send} title={open ? undefined : "Connect first"} />
      </div>
      <div className="ws-composer-editor">
        <CodeEditor
          language={LANGUAGE[format]}
          ariaLabel="Message"
          value={text}
          variables={props.variables}
          onChange={props.onText}
        />
      </div>
      {warning ? <p role="status" className="ws-warning">{warning}</p> : null}
      {blocking ? <p role="alert" className="field-error">{blocking}</p> : null}
      <div className="ws-composer-bar">
        <input aria-label="Template name" placeholder="Template name" value={name} onChange={(e) => setName(e.target.value)} />
        <Button
          label={replacing ? "Update template" : "Save as template"}
          disabled={nameError !== null || blocking !== null}
          title={nameError ?? undefined}
          onClick={() => props.onSaveTemplate(name)}
        />
      </div>
    </div>
  );
}
