import type { RequestKind } from "../../ipc/bindings";
import type { TabItem } from "../../ui/Tabs";

interface Counts {
  readonly params: number;
  readonly headers: number;
}

/** The request sub-tabs for a kind (plan 06 §9.2, §9.3; plan 07 §8; plan 08 §7). */
export function sectionsFor(kind: RequestKind, counts: Counts): readonly TabItem[] {
  const params = { id: "params", label: "Params", count: counts.params };
  const headers = { id: "headers", label: "Headers", count: counts.headers };
  if (kind === "ws") {
    const messages = { id: "messages", label: "Messages" };
    const subprotocols = { id: "subprotocols", label: "Subprotocols" };
    return [params, headers, messages, subprotocols, { id: "auth", label: "Auth" }, { id: "settings", label: "Settings" }];
  }
  if (kind === "grpc") {
    const metadata = { id: "metadata", label: "Metadata", count: counts.headers };
    return [{ id: "grpc-message", label: "Message" }, metadata, { id: "protos", label: "Protos" }, { id: "auth", label: "Auth" }, { id: "settings", label: "Settings" }];
  }
  const tail = [
    { id: "auth", label: "Auth" },
    { id: "scripts", label: "Scripts" },
    { id: "settings", label: "Settings" },
  ];
  if (kind === "graphql") return [params, headers, { id: "query", label: "Query" }, { id: "variables", label: "Variables" }, { id: "subscription", label: "Subscription" }, ...tail];
  if (kind === "soap") return [params, headers, { id: "body", label: "Body" }, { id: "soap", label: "SOAP" }, ...tail];
  return [params, headers, { id: "body", label: "Body" }, ...tail];
}
