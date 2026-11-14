import type { Body, EffectiveVariable, RequestLocation } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { CodeEditor } from "../../ui/code/CodeEditor";
import { reportError } from "../../ui/toastStore";
import { pickBodyFile } from "./api";
import { fromRows, toRows } from "./content";
import { KeyValueTable } from "./KeyValueTable";
import { MultipartEditor } from "./MultipartEditor";
import { formatXmlText } from "./xmlFormat";

export interface BodyEditorProps {
  readonly body: Body;
  readonly onChange: (body: Body) => void;
  /** Where the request lives: picked files are granted to that location only. */
  readonly location: RequestLocation;
  readonly variables?: readonly EffectiveVariable[];
  /** Body types the type select offers; all of them when omitted (SOAP passes `["xml"]`). */
  readonly allowedTypes?: readonly Body["type"][];
}

type TextType = "json" | "xml" | "text";
const isTextBody = (body: Body): body is Extract<Body, { type: TextType }> =>
  body.type === "json" || body.type === "xml" || body.type === "text";

const EMPTY: Record<string, Body> = {
  none: { type: "none" },
  form: { type: "form", fields: [] },
  multipart: { type: "multipart", parts: [] },
  binary: { type: "binary", file: null },
};

/** Switching between JSON, XML and Text keeps the text. */
export function bodyOfType(current: Body, type: string): Body {
  if (type === "json" || type === "xml" || type === "text") {
    return { type, text: isTextBody(current) ? current.text : "" };
  }
  return EMPTY[type] ?? EMPTY.none;
}

/** Re-indents valid JSON; invalid input is returned as is. */
export function formatJsonText(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}

const BODY_TYPE_OPTIONS: readonly { value: Body["type"]; label: string }[] = [
  { value: "none", label: "None" },
  { value: "json", label: "JSON" },
  { value: "xml", label: "XML" },
  { value: "text", label: "Text" },
  { value: "form", label: "Form" },
  { value: "multipart", label: "Multipart" },
  { value: "binary", label: "Binary" },
];

export function BodyEditor({ body, onChange, location, variables, allowedTypes }: BodyEditorProps) {
  if (body.type === "unsupported") {
    return <p className="empty-text">Made by a newer Papan. This body is read-only.</p>;
  }

  const chooseBinary = async () => {
    try {
      const picked = await pickBodyFile(location);
      if (picked) onChange({ type: "binary", file: picked.path });
    } catch (err) {
      reportError(err);
    }
  };

  return (
    <div className="editor-form">
      <label>
        Body type
        <select aria-label="Body type" value={body.type} onChange={(e) => onChange(bodyOfType(body, e.target.value))}>
          {BODY_TYPE_OPTIONS.filter((o) => !allowedTypes || allowedTypes.includes(o.value)).map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      </label>
      {isTextBody(body) ? (
        <>
          {body.type === "json" ? (
            <Button label="Format" onClick={() => onChange({ ...body, text: formatJsonText(body.text) })} />
          ) : null}
          {body.type === "xml" ? (
            <Button label="Format" onClick={() => onChange({ ...body, text: formatXmlText(body.text) })} />
          ) : null}
          <div className="body-text">
            <CodeEditor
              language={body.type}
              ariaLabel="Body text"
              value={body.text}
              variables={variables}
              onChange={(text) => onChange({ ...body, text })}
            />
          </div>
        </>
      ) : null}
      {body.type === "form" ? (
        <KeyValueTable label="Form fields" variables={variables} rows={toRows(body.fields)} onChange={(rows) => onChange({ type: "form", fields: fromRows(rows) })} />
      ) : null}
      {body.type === "multipart" ? (
        <MultipartEditor location={location} parts={body.parts} onChange={(parts) => onChange({ type: "multipart", parts: [...parts] })} />
      ) : null}
      {body.type === "binary" ? (
        <div>
          <span title={body.file ?? undefined}>{body.file ? body.file.split(/[\\/]/).pop() : "No file chosen"}</span>
          <Button label="Choose file…" onClick={() => void chooseBinary()} />
        </div>
      ) : null}
    </div>
  );
}
