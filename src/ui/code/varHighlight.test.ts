import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { describe, expect, it } from "vitest";
import type { EditorVariable } from "./types";
import { classifyToken, MISSING_TEXT, SECRET_MASK, tooltipText, varHighlight, variablesFacet } from "./varHighlight";

const variables: readonly EditorVariable[] = [
  { name: "host", value: "example.com", source: "environment", secret: false, resolved: true },
  { name: "token", value: "", source: "collection", secret: true, resolved: true },
  { name: "$uuid", value: "", source: "dynamic", secret: false, resolved: true },
  { name: "off", value: "", source: "global", secret: false, resolved: false },
];

function classesFor(doc: string, vars: readonly EditorVariable[] | undefined): string[] {
  const state = EditorState.create({ doc, extensions: [variablesFacet.of(vars), varHighlight] });
  const found: string[] = [];
  for (const source of state.facet(EditorView.decorations)) {
    const set = typeof source === "function" ? source(null as unknown as EditorView) : source;
    set.between(0, doc.length, (_f, _t, v) => {
      found.push(String(v.spec.class));
    });
  }
  return found;
}

describe("varHighlight", () => {
  it.each([
    ["host", "var-ok"],
    ["token", "var-secret"],
    ["$uuid", "var-dynamic"],
    ["off", "var-missing"],
    ["nope", "var-missing"],
  ])("classifies %s as %s", (name, expected) => {
    expect(classifyToken(name, variables)).toBe(expected);
  });

  it("decorates each token in the document", () => {
    expect(classesFor("{{host}}/{{token}}/{{nope}}/\\{{host}}", variables)).toEqual([
      "var-ok",
      "var-secret",
      "var-missing",
    ]);
  });

  it("decorates nothing without variables", () => {
    expect(classesFor("{{host}}", undefined)).toEqual([]);
  });

  it("describes value and source, masks secrets, and names the missing", () => {
    expect(tooltipText("host", variables)).toBe("example.com (environment)");
    expect(tooltipText("token", variables)).toBe(`${SECRET_MASK} (collection)`);
    expect(tooltipText("$uuid", variables)).toBe("$uuid (dynamic)");
    expect(tooltipText("nope", variables)).toBe(MISSING_TEXT);
  });
});
