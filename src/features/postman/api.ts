import { commands } from "../../ipc/bindings";
import type {
  ExportOptions,
  ImportOptions,
  PostmanExportResult,
  PostmanImportResult,
  PostmanPreview,
} from "../../ipc/bindings";

// Thin wrappers over the generated bindings; unit tests `vi.mock` this module.
/** Native file picker in Rust; null when the user cancels. */
export const importPick = (): Promise<PostmanPreview | null> => commands.postmanImportPick();
/** Null when the user cancels the folder picker of a new-collection import. */
export const importCommit = (token: string, options: ImportOptions): Promise<PostmanImportResult | null> =>
  commands.postmanImportCommit(token, options);
export const importDiscard = (token: string): Promise<null> => commands.postmanImportDiscard(token);
/** Null when the user cancels the folder picker. */
export const exportCollection = (options: ExportOptions): Promise<PostmanExportResult | null> =>
  commands.postmanExport(options);
