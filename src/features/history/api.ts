import { commands } from "../../ipc/bindings";
import type { HistoryEntry, ScratchItem } from "../../ipc/bindings";

// Thin wrappers over the generated bindings; unit tests `vi.mock` this module.
export const listHistory = (): Promise<HistoryEntry[]> => commands.listHistory();
export const clearHistory = (): Promise<null> => commands.clearHistory();
export const restoreHistoryEntry = (id: string): Promise<ScratchItem> =>
  commands.restoreHistoryEntry(id);
