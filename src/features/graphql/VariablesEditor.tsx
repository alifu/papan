import type { EffectiveVariable } from "../../ipc/bindings";
import { CodeEditor } from "../../ui/code/CodeEditor";

export interface VariablesEditorProps {
  readonly value: string;
  readonly onChange: (value: string) => void;
  readonly variables?: readonly EffectiveVariable[];
  /** Names the editor in its aria label and warnings; defaults to "GraphQL variables". */
  readonly label?: string;
}

/** An advisory message when the text is not a JSON object; the send itself decides in Rust. */
export function variablesWarning(text: string, noun = "Variables"): string | null {
  if (text.trim() === "") return null;
  try {
    const parsed: unknown = JSON.parse(text);
    const isObject = typeof parsed === "object" && parsed !== null && !Array.isArray(parsed);
    return isObject ? null : `${noun} should be a JSON object.`;
  } catch {
    return `${noun} ${noun === "Variables" ? "are" : "is"} not valid JSON.`;
  }
}

/** JSON editor for the `variables` of a GraphQL request. */
export function VariablesEditor({ value, onChange, variables, label }: VariablesEditorProps) {
  const warning = variablesWarning(value, label === undefined ? "Variables" : label);
  return (
    <div className="editor-form">
      {warning ? (
        <p role="status" className="graphql-warning">
          {warning}
        </p>
      ) : null}
      <div className="body-text">
        <CodeEditor language="json" ariaLabel={label ?? "GraphQL variables"} value={value} variables={variables} onChange={onChange} />
      </div>
    </div>
  );
}
