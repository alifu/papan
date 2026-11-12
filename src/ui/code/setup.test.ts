import { EditorState } from "@codemirror/state";
import { keymap } from "@codemirror/view";
import { describe, expect, it } from "vitest";
import { baseExtensions, editorKeymap, lineExtensions } from "./setup";

const boundKeys = (): string[] =>
  EditorState.create({ extensions: baseExtensions() })
    .facet(keymap)
    .flat()
    .flatMap((b) => [b.key, b.mac, b.win, b.linux])
    .filter((k): k is string => Boolean(k));

describe("editor setup", () => {
  it("leaves Mod-Enter and Mod-S to the app", () => {
    const keys = boundKeys();
    expect(keys).not.toContain("Mod-Enter");
    expect(keys).not.toContain("Mod-s");
    expect(editorKeymap.length).toBeGreaterThan(10);
  });

  it("strips line breaks from single-line transactions and keeps the cursor after the insert", () => {
    const state = EditorState.create({ doc: "ab", extensions: lineExtensions(true) });
    const next = state.update({ changes: { from: 1, insert: "x\ny\r\nz" }, selection: { anchor: 7 } }).state;
    expect(next.doc.toString()).toBe("axyzb");
    expect(next.selection.main.head).toBe(4);
  });

  it("keeps line breaks when not single-line", () => {
    const state = EditorState.create({ doc: "ab", extensions: lineExtensions(false) });
    expect(state.update({ changes: { from: 1, insert: "\n" } }).state.doc.lines).toBe(2);
  });
});
