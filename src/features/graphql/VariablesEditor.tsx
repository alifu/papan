import type { EffectiveVariable } from "../../ipc/bindings";
import { CodeEditor } from "../../ui/code/CodeEditor";

export interface VariablesEditorProps {
  readonly value: string;
  readonly onChange: (value: string) => void;
  readonly variables?: readonly EffectiveVariable[];
}

/** An advisory message when the text is not a JSON object; the send itself decides in Rust. */
export function variablesWarning(text: string): string | null {
  if (text.trim() === "") return null;
  try {
    const parsed: unknown = JSON.parse(text);
    const isObject = typeof parsed === "object" && parsed !== null && !Array.isArray(parsed);
    return isObject ? null : "Variables should be a JSON object.";
  } catch {
    return "Variables are not valid JSON.";
  }
}

/** JSON editor for the `variables` of a GraphQL request. */
export function VariablesEditor({ value, onChange, variables }: VariablesEditorProps) {
  const warning = variablesWarning(value);
  return (
    <div className="editor-form">
      {warning ? (
        <p role="status" className="graphql-warning">
          {warning}
        </p>
      ) : null}
      <div className="body-text">
        <CodeEditor language="json" ariaLabel="GraphQL variables" value={value} variables={variables} onChange={onChange} />
      </div>
    </div>
  );
}
