import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { EditorView } from "@codemirror/view";
import { tags as t } from "@lezer/highlight";

const MONO = 'ui-monospace, SFMono-Regular, Menlo, Consolas, "Courier New", monospace';

/** Built only from CSS variables, so light and dark follow the app theme. */
const highlightStyle = HighlightStyle.define([
  { tag: [t.propertyName, t.attributeName], color: "var(--syntax-key)" },
  { tag: [t.string, t.attributeValue], color: "var(--syntax-string)" },
  { tag: [t.number], color: "var(--syntax-number)" },
  { tag: [t.bool, t.null, t.keyword], color: "var(--syntax-keyword)" },
  { tag: [t.tagName, t.angleBracket], color: "var(--syntax-tag)" },
  { tag: [t.comment, t.docComment, t.processingInstruction], color: "var(--syntax-comment)" },
  { tag: [t.punctuation, t.separator, t.bracket], color: "var(--color-text)" },
]);

const editorTheme = EditorView.theme({
  "&": { color: "var(--color-text)", backgroundColor: "var(--color-content-bg)", height: "100%" },
  "&.cm-focused": { outline: "1px solid var(--color-focus)" },
  ".cm-scroller": { fontFamily: MONO, fontSize: "var(--font-size)", lineHeight: "var(--line-height)", overflow: "auto" },
  ".cm-content": { caretColor: "var(--color-text)", padding: "2px 0" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--color-text)" },
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground": {
    backgroundColor: "var(--color-selected-bg)",
    opacity: "0.35",
  },
  ".cm-matchingBracket": { outline: "1px solid var(--color-control-border)", backgroundColor: "transparent" },
  ".var-ok": { color: "var(--syntax-var-ok)", fontWeight: "bold" },
  ".var-secret": { color: "var(--syntax-var-secret)", fontWeight: "bold" },
  ".var-dynamic": { color: "var(--syntax-var-dynamic)", fontWeight: "bold" },
  ".var-missing": {
    color: "var(--color-error-text)",
    textDecoration: "underline wavy",
  },
  ".search-mark": { backgroundColor: "var(--color-splitter-hover)", color: "var(--color-text)" },
  ".search-mark-current": { backgroundColor: "var(--color-selected-bg)", color: "var(--color-selected-text)" },
  ".cm-tooltip": {
    border: "1px solid var(--color-control-border)",
    backgroundColor: "var(--color-content-bg)",
    color: "var(--color-text)",
    padding: "2px 6px",
    fontFamily: "var(--font-family)",
    fontSize: "var(--font-size)",
  },
});

export const editorThemeExtensions = [editorTheme, syntaxHighlighting(highlightStyle)];
export const singleLineTheme = EditorView.theme({
  "&": { height: "auto" },
  ".cm-scroller": { overflow: "hidden" },
  ".cm-line": { whiteSpace: "pre" },
});
