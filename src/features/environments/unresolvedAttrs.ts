import type { EffectiveVariable } from "../../ipc/bindings";
import { unresolvedIn } from "./template";

/** Class and title for a plain input whose text names variables that do not resolve. */
export function unresolvedAttrs(
  text: string,
  variables: readonly EffectiveVariable[] | undefined,
): { className?: string; title?: string } {
  if (!variables) return {};
  const names = unresolvedIn(text, variables);
  if (names.length === 0) return {};
  return { className: "has-unresolved", title: `Not defined: ${names.join(", ")}` };
}
