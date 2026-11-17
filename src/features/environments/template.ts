/**
 * Template grammar, identical to the Rust resolver (`src-tauri/src/vars/template.rs`):
 * a token is `{{`, optional spaces, a name, optional spaces, `}}`. The name is 1-128 characters
 * (code points) with no `{`, `}`, CR or LF, and the first `}}` closes. `\{{` is an escape that
 * emits a literal `{{`. Anything else is literal. Offsets are UTF-16 code units.
 */

export const MAX_NAME_LENGTH = 128;

export interface TemplateToken {
  readonly from: number;
  readonly to: number;
  readonly name: string;
}

/** The part of an effective variable the frontend needs to judge a name. */
export interface KnownVariable {
  readonly name: string;
  readonly resolved: boolean;
}

const FORBIDDEN_IN_NAME = /[{}\r\n]/;

function isValidName(name: string): boolean {
  return (
    name.length > 0 &&
    [...name].length <= MAX_NAME_LENGTH &&
    !FORBIDDEN_IN_NAME.test(name)
  );
}

/** Finds every variable token in `text`, in order. Pure and total. */
export function findTokens(text: string): readonly TemplateToken[] {
  const tokens: TemplateToken[] = [];
  let i = 0;
  while (i < text.length) {
    if (text.startsWith("\\{{", i)) {
      i += 3;
      continue;
    }
    if (!text.startsWith("{{", i)) {
      i += 1;
      continue;
    }
    const close = text.indexOf("}}", i + 2);
    if (close < 0) break;
    const name = text.slice(i + 2, close).replace(/^ +| +$/g, "");
    if (isValidName(name)) {
      tokens.push({ from: i, to: close + 2, name });
      i = close + 2;
    } else {
      i += 1;
    }
  }
  return tokens;
}

/** Distinct names used in `text` that no resolved variable defines, in order of first use. */
export function unresolvedIn(text: string, variables: readonly KnownVariable[]): readonly string[] {
  const known = new Set(variables.filter((v) => v.resolved).map((v) => v.name));
  const missing = new Set<string>();
  for (const { name } of findTokens(text)) {
    if (!known.has(name)) missing.add(name);
  }
  return [...missing];
}
