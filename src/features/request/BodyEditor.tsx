import type { Body, RequestLocation } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { reportError } from "../../ui/toastStore";
import { pickBodyFile } from "./api";
import { fromRows, toRows } from "./content";
import { KeyValueTable } from "./KeyValueTable";
import { MultipartEditor } from "./MultipartEditor";

export interface BodyEditorProps {
  readonly body: Body;
  readonly onChange: (body: Body) => void;
  /** Where the request lives: picked files are granted to that location only. */
  readonly location: RequestLocation;
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

export function BodyEditor({ body, onChange, location }: BodyEditorProps) {
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
          <option value="none">None</option>
          <option value="json">JSON</option>
          <option value="xml">XML</option>
          <option value="text">Text</option>
          <option value="form">Form</option>
          <option value="multipart">Multipart</option>
          <option value="binary">Binary</option>
        </select>
      </label>
      {isTextBody(body) ? (
        <>
          {body.type === "json" ? (
            <Button label="Format" onClick={() => onChange({ ...body, text: formatJsonText(body.text) })} />
          ) : null}
          <textarea
            aria-label="Body text"
            className="body-text"
            spellCheck={false}
            value={body.text}
            onChange={(e) => onChange({ ...body, text: e.target.value })}
          />
        </>
      ) : null}
      {body.type === "form" ? (
        <KeyValueTable label="Form fields" rows={toRows(body.fields)} onChange={(rows) => onChange({ type: "form", fields: fromRows(rows) })} />
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
