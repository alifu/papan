import { commands } from "../../ipc/bindings";
import type { RequestKind, ScratchItem, TreeChange } from "../../ipc/bindings";

// Thin wrappers over the generated bindings; unit tests `vi.mock` this module.
export const listScratch = (): Promise<ScratchItem[]> => commands.listScratch();
export const createScratch = (kind: RequestKind = "http"): Promise<ScratchItem> => commands.createScratch(kind);
export const renameScratch = (id: string, name: string): Promise<ScratchItem[]> =>
  commands.renameScratch(id, name);
export const deleteScratch = (id: string): Promise<ScratchItem[]> => commands.deleteScratch(id);
export const moveScratchToCollection = (
  id: string,
  collectionId: string,
  targetFolder: string,
): Promise<TreeChange> => commands.moveScratchToCollection(id, collectionId, targetFolder);
