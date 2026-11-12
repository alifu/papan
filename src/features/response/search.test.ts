import { describe, expect, it } from "vitest";
import { findMatches, MAX_MATCHES, stepIndex } from "./search";

describe("findMatches", () => {
  it("finds case-insensitive literal matches", () => {
    const { matches } = findMatches("Foo foo FOO", "foo");
    expect(matches).toEqual([
      { start: 0, end: 3 },
      { start: 4, end: 7 },
      { start: 8, end: 11 },
    ]);
  });
  it("treats regex characters literally and ignores an empty query", () => {
    expect(findMatches("a.c abc", ".").matches).toHaveLength(1);
    expect(findMatches("abc", "").matches).toEqual([]);
  });
  it("caps the list", () => {
    const result = findMatches("a".repeat(MAX_MATCHES + 5), "a");
    expect(result.matches).toHaveLength(MAX_MATCHES);
    expect(result.capped).toBe(true);
  });
});

describe("stepIndex", () => {
  it("wraps in both directions and survives zero matches", () => {
    expect(stepIndex(2, 3, 1)).toBe(0);
    expect(stepIndex(0, 3, -1)).toBe(2);
    expect(stepIndex(0, 0, 1)).toBe(0);
  });
});
