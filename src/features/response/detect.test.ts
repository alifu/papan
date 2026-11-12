import { describe, expect, it } from "vitest";
import { availableModes, detectKind } from "./detect";

describe("detectKind", () => {
  it.each([
    ["application/json; charset=utf-8", "json"],
    ["application/problem+json", "json"],
    ["text/html", "html"],
    ["application/xml", "xml"],
    ["application/atom+xml", "xml"],
    ["image/png", "image"],
    ["text/css", "text"],
  ])("maps %s to %s", (type, kind) => {
    expect(detectKind(type)).toBe(kind);
  });
  it("sniffs JSON and XML when the type is missing or text/plain", () => {
    expect(detectKind(null, ' {"a":1}')).toBe("json");
    expect(detectKind("text/plain", "[1]")).toBe("json");
    expect(detectKind(undefined, '<?xml version="1.0"?><a/>')).toBe("xml");
    expect(detectKind(null, "hello")).toBe("text");
  });
  it("offers modes per kind", () => {
    expect(availableModes("json")).toEqual(["pretty", "raw", "tree"]);
    expect(availableModes("html")).toContain("preview");
    expect(availableModes("image")).toEqual(["preview"]);
    expect(availableModes("text")).toEqual(["raw"]);
  });
});
