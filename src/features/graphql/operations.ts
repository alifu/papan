import { Kind, parse, type OperationDefinitionNode } from "graphql";
import type { RequestContent } from "../../ipc/bindings";

export type OperationType = "query" | "mutation" | "subscription";

export interface GraphqlOperation {
  /** Empty for an anonymous operation. */
  readonly name: string;
  readonly type: OperationType;
}

export const SUBSCRIPTION_HINT = "Subscriptions arrive with the WebSocket engine (M6b).";

function definitions(query: string): readonly OperationDefinitionNode[] {
  try {
    return parse(query, { noLocation: true }).definitions.filter(
      (d): d is OperationDefinitionNode => d.kind === Kind.OPERATION_DEFINITION,
    );
  } catch {
    return [];
  }
}

/** Operations of a document; a document that does not parse has none. */
export function listOperations(query: string): readonly GraphqlOperation[] {
  return definitions(query).map((d) => ({ name: d.name?.value ?? "", type: d.operation }));
}

/** The operation a send would run: the named one, or the only one when no name matches. */
export function selectedOperation(query: string, operationName: string): GraphqlOperation | null {
  const all = listOperations(query);
  const named = all.find((o) => o.name === operationName);
  if (named) return named;
  // A stale name (the operation was renamed or removed) falls back to the only operation.
  return all.length === 1 ? (all[0] ?? null) : null;
}

/** True when the request would run a subscription, which is not sent in M6. */
export function runsSubscription(content: RequestContent): boolean {
  const { body } = content;
  if (content.kind !== "graphql" || body.type !== "graphql") return false;
  return selectedOperation(body.query, body.operation_name)?.type === "subscription";
}
