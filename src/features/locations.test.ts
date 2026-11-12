import { describe, expect, it } from "vitest";
import { collectionLocation as c, isWithin, locationWithin, relocate, sameLocation, scratchLocation as s } from "./locations";

describe("locations", () => {
  it("sameLocation compares scope and ids", () => {
    expect(sameLocation(s("a"), s("a"))).toBe(true);
    expect(sameLocation(s("a"), s("b"))).toBe(false);
    expect(sameLocation(s("a"), c("x", "a"))).toBe(false);
    expect(sameLocation(c("x", "a"), c("x", "a"))).toBe(true);
    expect(sameLocation(c("x", "a"), c("y", "a"))).toBe(false);
    expect(sameLocation(c("x", "a"), s("a"))).toBe(false);
  });

  it("isWithin respects segment boundaries", () => {
    expect(isWithin("auth", "auth/a")).toBe(true);
    expect(isWithin("auth", "authors/a")).toBe(false);
    expect(isWithin("", "anything")).toBe(true);
  });

  it("locationWithin and relocate map below a folder", () => {
    expect(locationWithin(c("x", "a"), c("y", "a/b"))).toBe(false);
    expect(locationWithin(c("x", "a"), s("a"))).toBe(false);
    expect(relocate(c("x", "a"), c("x", "z"), c("x", "a/b"))).toEqual(c("x", "z/b"));
    expect(relocate(c("x", "a"), c("x", "z"), c("x", "q"))).toBeNull();
    expect(relocate(s("a"), c("x", "p"), s("a"))).toEqual(c("x", "p"));
  });
});
