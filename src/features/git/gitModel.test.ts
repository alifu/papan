import { describe, expect, it } from "vitest";
import {
  changeLabel,
  conflictsOf,
  diffLineClass,
  syncSummary,
} from "./gitModel";

const file = (index: string, worktree: string, conflicted = false) => ({
  path: "a",
  index,
  worktree,
  conflicted,
});

describe("gitModel", () => {
  it("labels changes", () => {
    expect(changeLabel(file("?", "?"))).toBe("New");
    expect(changeLabel(file(".", "M"))).toBe("Modified");
    expect(changeLabel(file("A", "."))).toBe("Added");
    expect(changeLabel(file("U", "U", true))).toBe("Conflict");
    expect(changeLabel(file("X", "."))).toBe("Changed");
  });
  it("finds conflicts", () => {
    expect(conflictsOf([file("M", "."), file("U", "U", true)])).toHaveLength(1);
  });
  it("summarises ahead/behind", () => {
    expect(syncSummary(2, 1)).toBe("↑2 ↓1");
    expect(syncSummary(0, 3)).toBe("↓3");
    expect(syncSummary(0, 0)).toBe("");
  });
  it("classes diff lines", () => {
    expect(diffLineClass("+x")).toBe("git-diff-add");
    expect(diffLineClass("-x")).toBe("git-diff-del");
    expect(diffLineClass("+++ b/x")).toBe("git-diff-meta");
    expect(diffLineClass("@@ -1 +1 @@")).toBe("git-diff-hunk");
    expect(diffLineClass(" ctx")).toBe("");
  });
});
