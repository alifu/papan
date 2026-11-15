import { commands } from "../../ipc/bindings";
import type {
  CollectionTree,
  ConflictSide,
  GitBranch,
  GitOutput,
  GitStatus,
} from "../../ipc/bindings";

// Thin wrappers over the generated bindings; unit tests `vi.mock` this module.
export const status = (id: string): Promise<GitStatus> =>
  commands.gitStatus(id);
export const init = (id: string): Promise<GitStatus> => commands.gitInit(id);
/** Null when the user cancels the folder picker. */
export const clone = (url: string): Promise<CollectionTree | null> =>
  commands.gitClone(url);
export const diff = (id: string, path: string): Promise<string> =>
  commands.gitDiff(id, path);
export const commit = (id: string, message: string): Promise<GitOutput> =>
  commands.gitCommit(id, message);
export const pull = (id: string): Promise<GitOutput> => commands.gitPull(id);
export const push = (id: string): Promise<GitOutput> => commands.gitPush(id);
export const branches = (id: string): Promise<GitBranch[]> =>
  commands.gitBranches(id);
export const switchBranch = (id: string, branch: string): Promise<GitStatus> =>
  commands.gitSwitch(id, branch);
export const createBranch = (id: string, branch: string): Promise<GitStatus> =>
  commands.gitCreateBranch(id, branch);
export const resolve = (
  id: string,
  path: string,
  side: ConflictSide,
): Promise<GitStatus> => commands.gitResolve(id, path, side);
