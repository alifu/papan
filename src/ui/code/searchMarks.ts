import { type EditorState, type Extension, StateEffect, StateField } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView } from "@codemirror/view";
import type { EditorMarks } from "./types";

const setMarks = StateEffect.define<EditorMarks>();

const markDeco = Decoration.mark({ class: "search-mark" });
const currentDeco = Decoration.mark({ class: "search-mark-current" });

function build(marks: EditorMarks, length: number): DecorationSet {
  const ranges = marks.ranges
    .filter((r) => r.from < r.to && r.to <= length)
    .map((r, i) => ({ r, i }))
    .map(({ r, i }) => (i === marks.current ? currentDeco : markDeco).range(r.from, r.to));
  return Decoration.set(ranges, true);
}

const marksField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, tr) {
    for (const effect of tr.effects) {
      if (effect.is(setMarks)) return build(effect.value, tr.state.doc.length);
    }
    return tr.docChanged ? value.map(tr.changes) : value;
  },
  provide: (field) => EditorView.decorations.from(field),
});

export const searchMarks: Extension = marksField;

/** Effects that replace the marks and scroll the current match into view. */
export function marksEffects(marks: EditorMarks): StateEffect<unknown>[] {
  const effects: StateEffect<unknown>[] = [setMarks.of(marks)];
  const current = marks.current === null ? undefined : marks.ranges[marks.current];
  if (current) effects.push(EditorView.scrollIntoView(current.from, { y: "center" }));
  return effects;
}

/** The decorated ranges as `[from, to, class]` (for tests and debugging). */
export function readMarks(state: EditorState): ReadonlyArray<readonly [number, number, string]> {
  const out: Array<readonly [number, number, string]> = [];
  state.field(marksField).between(0, state.doc.length, (from, to, value) => {
    out.push([from, to, String(value.spec.class)]);
  });
  return out;
}
