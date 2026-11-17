export type VariableScope = "env" | "collection" | "global";

export const SCOPE_TARGETS: Readonly<Record<VariableScope, string>> = {
  env: "environment",
  collection: "collectionVariables",
  global: "globals",
};

const IDENTIFIER = /^[A-Za-z_$][A-Za-z0-9_$]*$/;

const segment = (part: string | number): string =>
  typeof part === "number" ? `[${part}]` : IDENTIFIER.test(part) ? `.${part}` : `[${JSON.stringify(part)}]`;

/** One post-response script line storing the value at `path` of the JSON body into a variable. */
export function saveToVariableSnippet(path: readonly (string | number)[], name: string, scope: VariableScope): string {
  const access = path.map(segment).join("");
  return `pm.${SCOPE_TARGETS[scope]}.set(${JSON.stringify(name)}, pm.response.json()${access});`;
}
