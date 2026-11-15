import type { GitFileChange } from "../../ipc/bindings";

const LABELS: Readonly<Record<string, string>> = {
  M: "Modified",
  A: "Added",
  D: "Deleted",
  R: "Renamed",
  C: "Copied",
  T: "Type changed",
  "?": "New",
};

/** Human label for a changed file; conflicts win over the letters. */
export function changeLabel(file: GitFileChange): string {
  if (file.conflicted) return "Conflict";
  const letter = file.index !== "." ? file.index : file.worktree;
  return LABELS[letter] ?? "Changed";
}

export const conflictsOf = (
  files: readonly GitFileChange[],
): readonly GitFileChange[] => files.filter((f) => f.conflicted);

/** "↑2 ↓1", or "" when level with the remote. */
export function syncSummary(ahead: number, behind: number): string {
  return [ahead > 0 ? `↑${ahead}` : "", behind > 0 ? `↓${behind}` : ""]
    .filter(Boolean)
    .join(" ");
}

/** CSS class for a diff line, so added/removed lines can be coloured. */
export function diffLineClass(line: string): string {
  if (line.startsWith("+++") || line.startsWith("---")) return "git-diff-meta";
  if (line.startsWith("+")) return "git-diff-add";
  if (line.startsWith("-")) return "git-diff-del";
  if (line.startsWith("@@")) return "git-diff-hunk";
  return "";
}
