import { commands } from "../../ipc/bindings";
import type { GraphqlSchema, RequestContent, RequestLocation } from "../../ipc/bindings";

// Thin wrappers over the generated bindings; unit tests `vi.mock` this module.
export const introspectGraphql = (
  requestId: string,
  location: RequestLocation,
  content: RequestContent,
): Promise<GraphqlSchema> => commands.introspectGraphql(requestId, location, content);
export const readCachedSchema = (location: RequestLocation, content: RequestContent): Promise<GraphqlSchema | null> =>
  commands.readCachedSchema(location, content);
export const loadSchemaFile = (location: RequestLocation, content: RequestContent): Promise<GraphqlSchema | null> =>
  commands.loadSchemaFile(location, content);
export const clearCachedSchema = (location: RequestLocation, content: RequestContent): Promise<null> =>
  commands.clearCachedSchema(location, content);
export const cancelIntrospection = (requestId: string): Promise<null> => commands.cancelRequest(requestId);
