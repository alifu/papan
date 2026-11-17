import { buildClientSchema, buildSchema, type GraphQLSchema, type IntrospectionQuery } from "graphql";
import type { GraphqlSchema } from "../../ipc/bindings";

const cache = new WeakMap<GraphqlSchema, GraphQLSchema | Error>();

function build(schema: GraphqlSchema): GraphQLSchema {
  const { source } = schema;
  if (source.type === "sdl") return buildSchema(source.text);
  const parsed = JSON.parse(source.json) as { data?: IntrospectionQuery } & Partial<IntrospectionQuery>;
  return buildClientSchema(parsed.data ?? (parsed as IntrospectionQuery));
}

/**
 * Builds the executable schema for a cached one (introspection JSON or SDL).
 * Returns the error instead of throwing; results are memoised per schema object (the store keeps one per fetch).
 */
export function toSchema(schema: GraphqlSchema): GraphQLSchema | Error {
  const known = cache.get(schema);
  if (known) return known;
  let result: GraphQLSchema | Error;
  try {
    result = build(schema);
  } catch (err) {
    result = err instanceof Error ? err : new Error("The schema could not be read.");
  }
  cache.set(schema, result);
  return result;
}
