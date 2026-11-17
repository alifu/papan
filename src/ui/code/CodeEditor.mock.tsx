import type { CodeEditorProps } from "./CodeEditor";

/** `<textarea>` stand-in for component tests: `vi.mock("../../ui/code/CodeEditor", () => import("../../ui/code/CodeEditor.mock"))`. */
export function CodeEditor({ value, onChange, ariaLabel, readOnly }: CodeEditorProps) {
  return (
    <textarea
      aria-label={ariaLabel}
      readOnly={readOnly}
      value={value}
      onChange={(e) => onChange?.(e.target.value)}
    />
  );
}
