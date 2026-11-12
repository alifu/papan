import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { marksEffects, readMarks, searchMarks } from "./searchMarks";

const make = () => EditorState.create({ doc: "aa bb aa", extensions: searchMarks });

describe("searchMarks", () => {
  it("marks every range and flags the current one", () => {
    const state = make().update({
      effects: marksEffects({ ranges: [{ from: 0, to: 2 }, { from: 6, to: 8 }], current: 1 }),
    }).state;
    expect(readMarks(state)).toEqual([
      [0, 2, "search-mark"],
      [6, 8, "search-mark-current"],
    ]);
  });

  it("clears marks and ignores ranges outside the document", () => {
    let state = make().update({
      effects: marksEffects({ ranges: [{ from: 0, to: 2 }, { from: 7, to: 99 }], current: null }),
    }).state;
    expect(readMarks(state)).toEqual([[0, 2, "search-mark"]]);
    state = state.update({ effects: marksEffects({ ranges: [], current: null }) }).state;
    expect(readMarks(state)).toEqual([]);
  });

  it("maps marks through document edits", () => {
    let state = make().update({ effects: marksEffects({ ranges: [{ from: 6, to: 8 }], current: 0 }) }).state;
    state = state.update({ changes: { from: 0, insert: "zz" } }).state;
    expect(readMarks(state)).toEqual([[8, 10, "search-mark-current"]]);
  });
});
