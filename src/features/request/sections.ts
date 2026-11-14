import type { RequestKind } from "../../ipc/bindings";
import type { TabItem } from "../../ui/Tabs";

interface Counts {
  readonly params: number;
  readonly headers: number;
}

/** The request sub-tabs for a kind (plan 06 §9.2, §9.3). */
export function sectionsFor(kind: RequestKind, counts: Counts): readonly TabItem[] {
  const params = { id: "params", label: "Params", count: counts.params };
  const headers = { id: "headers", label: "Headers", count: counts.headers };
  const tail = [
    { id: "auth", label: "Auth" },
    { id: "scripts", label: "Scripts" },
    { id: "settings", label: "Settings" },
  ];
  if (kind === "graphql") return [params, headers, { id: "query", label: "Query" }, { id: "variables", label: "Variables" }, ...tail];
  if (kind === "soap") return [params, headers, { id: "body", label: "Body" }, { id: "soap", label: "SOAP" }, ...tail];
  return [params, headers, { id: "body", label: "Body" }, ...tail];
}
