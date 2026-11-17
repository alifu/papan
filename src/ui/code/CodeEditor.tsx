import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { xml } from "@codemirror/lang-xml";
import { Annotation, Compartment, EditorState, type Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { graphql } from "cm6-graphql";
import { useEffect, useRef } from "react";
import { marksEffects, searchMarks } from "./searchMarks";
import { baseExtensions, lineExtensions } from "./setup";
import { editorThemeExtensions, singleLineTheme } from "./theme";
import type { EditorLanguage, EditorMarks, EditorVariable } from "./types";
import { varHighlight, variablesFacet } from "./varHighlight";

export interface CodeEditorProps {
  readonly value: string;
  readonly onChange?: (value: string) => void;
  readonly language: EditorLanguage;
  readonly readOnly?: boolean;
  readonly singleLine?: boolean;
  readonly ariaLabel: string;
  readonly variables?: readonly EditorVariable[];
  readonly marks?: EditorMarks;
  /** Extra extensions (e.g. a schema-aware graphql language), swapped through a compartment. */
  readonly extensions?: Extension;
}

/** Marks a transaction that applies a prop, so it is never echoed back through `onChange`. */
const external = Annotation.define<boolean>();

function languageExtension(language: EditorLanguage): Extension {
  if (language === "json") return json();
  if (language === "xml") return xml();
  if (language === "javascript") return javascript();
  if (language === "graphql") return graphql();
  return [];
}

function modeExtensions(readOnly: boolean, singleLine: boolean): Extension {
  return [EditorState.readOnly.of(readOnly), lineExtensions(singleLine), singleLine ? singleLineTheme : []];
}

function attributeExtensions(ariaLabel: string, singleLine: boolean): Extension {
  return EditorView.contentAttributes.of({
    "aria-label": ariaLabel,
    "aria-multiline": String(!singleLine),
  });
}

/** Smallest single replacement turning `from` into `to`. */
function minimalChange(from: string, to: string): { from: number; to: number; insert: string } {
  let start = 0;
  const max = Math.min(from.length, to.length);
  while (start < max && from[start] === to[start]) start += 1;
  let endFrom = from.length;
  let endTo = to.length;
  while (endFrom > start && endTo > start && from[endFrom - 1] === to[endTo - 1]) {
    endFrom -= 1;
    endTo -= 1;
  }
  return { from: start, to: endFrom, insert: to.slice(start, endTo) };
}

/** The one shared CodeMirror 6 editor: one `EditorView` per mount, props applied through compartments. */
export function CodeEditor(props: CodeEditorProps) {
  const { value, language, readOnly = false, singleLine = false, ariaLabel, variables, marks, extensions } = props;
  const host = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const onChange = useRef(props.onChange);
  onChange.current = props.onChange;

  const compartments = useRef({
    language: new Compartment(),
    mode: new Compartment(),
    aria: new Compartment(),
    variables: new Compartment(),
    extra: new Compartment(),
  });
  // Initial values are read once at mount; later changes flow through the effects below.
  const initial = useRef({ value, language, readOnly, singleLine, ariaLabel, variables, extensions });

  useEffect(() => {
    const c = compartments.current;
    const i = initial.current;
    const view = new EditorView({
      parent: host.current ?? undefined,
      state: EditorState.create({
        doc: i.value,
        extensions: [
          baseExtensions(),
          editorThemeExtensions,
          varHighlight,
          searchMarks,
          c.language.of(languageExtension(i.language)),
          c.mode.of(modeExtensions(i.readOnly, i.singleLine)),
          c.aria.of(attributeExtensions(i.ariaLabel, i.singleLine)),
          c.variables.of(variablesFacet.of(i.variables)),
          c.extra.of(i.extensions ?? []),
          EditorView.updateListener.of((update) => {
            if (!update.docChanged || update.transactions.some((tr) => tr.annotation(external))) return;
            onChange.current?.(update.state.doc.toString());
          }),
        ],
      }),
    });
    viewRef.current = view;
    return () => {
      view.destroy();
      viewRef.current = null;
    };
  }, []);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const current = view.state.doc.toString();
    if (current === value) return;
    view.dispatch({ changes: minimalChange(current, value), annotations: external.of(true) });
  }, [value]);

  useEffect(() => {
    viewRef.current?.dispatch({ effects: compartments.current.language.reconfigure(languageExtension(language)) });
  }, [language]);

  useEffect(() => {
    viewRef.current?.dispatch({ effects: compartments.current.mode.reconfigure(modeExtensions(readOnly, singleLine)) });
  }, [readOnly, singleLine]);

  useEffect(() => {
    viewRef.current?.dispatch({
      effects: compartments.current.aria.reconfigure(attributeExtensions(ariaLabel, singleLine)),
    });
  }, [ariaLabel, singleLine]);

  useEffect(() => {
    viewRef.current?.dispatch({ effects: compartments.current.variables.reconfigure(variablesFacet.of(variables)) });
  }, [variables]);

  useEffect(() => {
    viewRef.current?.dispatch({ effects: compartments.current.extra.reconfigure(extensions ?? []) });
  }, [extensions]);

  useEffect(() => {
    if (marks) viewRef.current?.dispatch({ effects: marksEffects(marks) });
  }, [marks]);

  return <div ref={host} className="code-editor" data-single-line={singleLine || undefined} />;
}
