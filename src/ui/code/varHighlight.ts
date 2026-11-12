import { type EditorState, Facet, StateField, type Extension } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, hoverTooltip } from "@codemirror/view";
import { findTokens } from "../../features/environments/template";
import type { EditorVariable } from "./types";

/** `undefined` turns highlighting off (read-only views); an array, even empty, turns it on. */
export const variablesFacet = Facet.define<readonly EditorVariable[] | undefined, readonly EditorVariable[] | undefined>({
  combine: (values) => values[values.length - 1],
});

export type TokenClass = "var-ok" | "var-secret" | "var-dynamic" | "var-missing";

export const SECRET_MASK = "•••••";
export const MISSING_TEXT = "Not defined in this scope";

export function findVariable(name: string, variables: readonly EditorVariable[]): EditorVariable | undefined {
  return variables.find((v) => v.name === name && v.resolved);
}

export function classifyToken(name: string, variables: readonly EditorVariable[]): TokenClass {
  const found = findVariable(name, variables);
  if (!found) return "var-missing";
  if (found.secret) return "var-secret";
  return found.source === "dynamic" ? "var-dynamic" : "var-ok";
}

export function tooltipText(name: string, variables: readonly EditorVariable[]): string {
  const found = findVariable(name, variables);
  if (!found) return MISSING_TEXT;
  const value = found.secret ? SECRET_MASK : found.value;
  return found.source === "dynamic" ? `${name} (dynamic)` : `${value} (${found.source})`;
}

function buildDecorations(state: EditorState): DecorationSet {
  const variables = state.facet(variablesFacet);
  if (!variables) return Decoration.none;
  const text = state.doc.toString();
  return Decoration.set(
    findTokens(text).map((token) =>
      Decoration.mark({ class: classifyToken(token.name, variables) }).range(token.from, token.to),
    ),
  );
}

const decorationsField = StateField.define<DecorationSet>({
  create: buildDecorations,
  update(value, tr) {
    const facetChanged = tr.startState.facet(variablesFacet) !== tr.state.facet(variablesFacet);
    return tr.docChanged || facetChanged ? buildDecorations(tr.state) : value;
  },
  provide: (field) => EditorView.decorations.from(field),
});

const hover = hoverTooltip((view, pos) => {
  const variables = view.state.facet(variablesFacet);
  if (!variables) return null;
  const token = findTokens(view.state.doc.toString()).find((tk) => pos >= tk.from && pos <= tk.to);
  if (!token) return null;
  return {
    pos: token.from,
    end: token.to,
    above: true,
    create() {
      const dom = document.createElement("div");
      dom.textContent = tooltipText(token.name, variables);
      return { dom };
    },
  };
});

export const varHighlight: Extension = [decorationsField, hover];
