import { describe, expect, it } from "vitest";
import { checkName } from "./nameSchema";

describe("checkName", () => {
  it("trims a valid name", () => {
    expect(checkName("  Auth  ")).toEqual({ ok: true, name: "Auth" });
  });

  it.each([
    ["", "Name is required."],
    ["   ", "Name is required."],
    ["a".repeat(101), "Name must be 100 characters or fewer."],
    ["bad\u0007name", "Name cannot contain control characters."],
  ])("rejects %j", (input, message) => {
    expect(checkName(input)).toEqual({ ok: false, message });
  });

  it("accepts exactly 100 characters and non-Latin names", () => {
    expect(checkName("a".repeat(100)).ok).toBe(true);
    expect(checkName("認証 🔐").ok).toBe(true);
  });
});
