import { describe, expect, it } from "vitest";
import { appendSnippet } from "./snippets";

describe("appendSnippet", () => {
  it("adds to an empty script", () => expect(appendSnippet("", "a();")).toBe("a();\n"));
  it("starts a new line when the text has none at its end", () => expect(appendSnippet("x();", "a();")).toBe("x();\na();\n"));
  it("keeps an existing trailing newline", () => expect(appendSnippet("x();\n", "a();")).toBe("x();\na();\n"));
});
