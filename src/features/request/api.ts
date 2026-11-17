import { commands } from "../../ipc/bindings";
import type {
  PickedFile,
  RequestContent,
  RequestLocation,
  ResponseBody,
  SendOutcome,
} from "../../ipc/bindings";

// Thin wrappers over the generated bindings; unit tests `vi.mock` this module.
// Full outcome with script reports; `meta` is null when a pre-request script stopped the send.
export const sendRequestOutcome = (
  requestId: string,
  location: RequestLocation,
  content: RequestContent,
): Promise<SendOutcome> => commands.sendRequest(requestId, location, content);
export const cancelRequest = (requestId: string): Promise<null> => commands.cancelRequest(requestId);
export const readResponseBody = (requestId: string): Promise<ResponseBody> =>
  commands.readResponseBody(requestId);
export const saveResponseBody = (requestId: string): Promise<string | null> =>
  commands.saveResponseBody(requestId);
export const pickBodyFile = (location: RequestLocation): Promise<PickedFile | null> =>
  commands.pickBodyFile(location);
export const clearCookies = (location: RequestLocation): Promise<null> => commands.clearCookies(location);
