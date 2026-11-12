import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { bracketMatching } from "@codemirror/language";
import { EditorSelection, EditorState, Transaction, type Extension } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";

/** Keys the app owns: Send (`Mod-Enter`, CodeMirror's "insert blank line") and Save. */
const APP_KEYS: ReadonlySet<string> = new Set(["Mod-Enter", "Mod-s", "Mod-S"]);

export const editorKeymap = [...defaultKeymap, ...historyKeymap].filter(
  (binding) => ![binding.key, binding.mac, binding.win, binding.linux].some((k) => k && APP_KEYS.has(k)),
);

const LINE_BREAKS = /\r\n|\r|\n/g;

/** Drops every line break from inserted text, keeping the cursor after what was inserted. */
const singleLineFilter = EditorState.transactionFilter.of((tr) => {
  if (!tr.docChanged) return tr;
  let multiline = false;
  tr.changes.iterChanges((_fa, _ta, _fb, _tb, inserted) => {
    if (inserted.lines > 1) multiline = true;
  });
  if (!multiline) return tr;

  const changes: { from: number; to: number; insert: string }[] = [];
  let shift = 0;
  let cursor = 0;
  tr.changes.iterChanges((from, to, _fb, _tb, inserted) => {
    const insert = inserted.toString().replace(LINE_BREAKS, "");
    changes.push({ from, to, insert });
    cursor = from + shift + insert.length;
    shift += insert.length - (to - from);
  });
  return { changes, selection: EditorSelection.cursor(cursor), userEvent: tr.annotation(Transaction.userEvent) };
});

/** Extensions shared by every editor; `singleLine` is applied through its own compartment. */
export function baseExtensions(): Extension {
  return [history(), bracketMatching(), keymap.of(editorKeymap)];
}

export function lineExtensions(singleLine: boolean): Extension {
  return singleLine ? singleLineFilter : EditorView.lineWrapping;
}
