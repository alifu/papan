import { commands } from "../../ipc/bindings";
import type { CurlImport, RequestContent, RequestLocation } from "../../ipc/bindings";

// Thin wrappers over the generated bindings; unit tests `vi.mock` this module.
export const exportCurl = (location: RequestLocation, content: RequestContent): Promise<string> =>
  commands.exportCurl(location, content);
export const importCurl = (text: string): Promise<CurlImport> => commands.importCurl(text);
