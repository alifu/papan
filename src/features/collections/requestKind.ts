import type { RequestKind } from "../../ipc/bindings";
import type { IconName } from "../../ui/icons";

export const REQUEST_KIND_OPTIONS: readonly { readonly kind: RequestKind; readonly label: string }[] = [
  { kind: "http", label: "HTTP" },
  { kind: "graphql", label: "GraphQL" },
  { kind: "soap", label: "SOAP" },
  { kind: "ws", label: "WebSocket" },
];

export function kindIcon(kind: RequestKind | null): IconName {
  if (kind === "graphql") return "request-graphql";
  if (kind === "soap") return "request-soap";
  if (kind === "ws") return "request-ws";
  return "request-http";
}

/** What the list badge shows: GraphQL and SOAP are always POST and WebSocket is always GET, so they show the protocol. */
export function kindBadge(kind: RequestKind | null, method: string | null): string | null {
  if (kind === "graphql") return "GQL";
  if (kind === "soap") return "SOAP";
  if (kind === "ws") return "WS";
  return method;
}
