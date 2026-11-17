import { describe, expect, it } from "vitest";
import { unresolvedAttrs } from "./unresolvedAttrs";

const vars = [{ name: "a", value: "1", source: "global" as const, secret: false, resolved: true }];

describe("unresolvedAttrs", () => {
  it("names the unresolved variables", () => {
    expect(unresolvedAttrs("{{a}}{{b}}{{c}}", vars)).toEqual({ className: "has-unresolved", title: "Not defined: b, c" });
  });
  it("is empty when all resolve, when there are no tokens, or before variables load", () => {
    expect(unresolvedAttrs("{{a}}", vars)).toEqual({});
    expect(unresolvedAttrs("plain", vars)).toEqual({});
    expect(unresolvedAttrs("{{x}}", undefined)).toEqual({});
  });
});
