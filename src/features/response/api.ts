import { commands } from "../../ipc/bindings";
import type { ResponseBody } from "../../ipc/bindings";

// Thin wrappers over the generated bindings; unit tests `vi.mock` this module.
export const readResponseBody = (requestId: string): Promise<ResponseBody> =>
  commands.readResponseBody(requestId);
export const saveResponseBody = (requestId: string): Promise<string | null> =>
  commands.saveResponseBody(requestId);
