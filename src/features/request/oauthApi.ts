import { commands } from "../../ipc/bindings";
import type { OAuthStatus, RequestContent, RequestLocation } from "../../ipc/bindings";

// Thin wrappers over the generated bindings; unit tests `vi.mock` this module.
export const oauthStatus = (location: RequestLocation, content: RequestContent): Promise<OAuthStatus> =>
  commands.oauthStatus(location, content);
export const oauthAuthorize = (
  flowId: string,
  location: RequestLocation,
  content: RequestContent,
  configKey: string,
): Promise<OAuthStatus> => commands.oauthAuthorize(flowId, location, content, configKey);
export const oauthCancel = (flowId: string): Promise<null> => commands.oauthCancel(flowId);
export const oauthClear = (location: RequestLocation, content: RequestContent): Promise<OAuthStatus> =>
  commands.oauthClear(location, content);
