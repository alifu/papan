export interface ScriptSnippet {
  readonly label: string;
  readonly code: string;
}

export const SNIPPETS: readonly ScriptSnippet[] = [
  { label: "Set environment variable", code: 'pm.environment.set("name", "value");' },
  { label: "Parse JSON response", code: "const data = pm.response.json();" },
  {
    label: "Test status 200",
    code: 'pm.test("Status is 200", function () {\n  pm.response.to.have.status(200);\n});',
  },
];

/** `text` with `code` on its own new line at the end. */
export function appendSnippet(text: string, code: string): string {
  if (text === "") return `${code}\n`;
  return `${text}${text.endsWith("\n") ? "" : "\n"}${code}\n`;
}
