/** Query parameter sync between the URL text and the params table (plan 03-http §3.1). Pure, immutable. */

export interface QueryPair {
  readonly name: string;
  readonly value: string;
  /** The source text had an `=` after the name (`flag=` vs `flag`); kept so joins do not rewrite it. */
  readonly hasEquals?: boolean;
}

export interface ParamRow extends QueryPair {
  readonly enabled: boolean;
}

export interface UrlParts {
  /** Everything before the first `?` or `#`. */
  readonly base: string;
  /** Text after the first `?` up to the first `#`; `null` when there is no `?`. */
  readonly query: string | null;
  /** Starts with `#` when present, otherwise empty. */
  readonly fragment: string;
}

export interface ParamsEdit {
  readonly url: string;
  readonly disabled_params: readonly QueryPair[];
}

/** Index just after the `{{…}}` token starting at `i`, or `i` when none starts there. */
function skipToken(text: string, i: number): number {
  if (!text.startsWith("{{", i)) return i;
  const close = text.indexOf("}}", i + 2);
  return close === -1 ? i : close + 2;
}

/** Index of the first of `chars` outside a `{{…}}` token, or -1. */
function indexOutsideTokens(text: string, chars: string, from = 0): number {
  let i = from;
  while (i < text.length) {
    const next = skipToken(text, i);
    if (next !== i) {
      i = next;
      continue;
    }
    if (chars.includes(text[i] ?? "")) return i;
    i += 1;
  }
  return -1;
}

export function splitUrl(url: string): UrlParts {
  const stop = indexOutsideTokens(url, "?#");
  if (stop === -1) return { base: url, query: null, fragment: "" };
  if (url[stop] === "#") return { base: url.slice(0, stop), query: null, fragment: url.slice(stop) };
  const hash = indexOutsideTokens(url, "#", stop + 1);
  return {
    base: url.slice(0, stop),
    query: url.slice(stop + 1, hash === -1 ? undefined : hash),
    fragment: hash === -1 ? "" : url.slice(hash),
  };
}

/** Splits at `&`, then each piece at its first `=`. No decoding; empty pieces yield no row. */
export function parseQuery(query: string): readonly QueryPair[] {
  const pairs: QueryPair[] = [];
  let start = 0;
  while (start <= query.length) {
    const amp = indexOutsideTokens(query, "&", start);
    const end = amp === -1 ? query.length : amp;
    const piece = query.slice(start, end);
    if (piece !== "") {
      const eq = indexOutsideTokens(piece, "=");
      pairs.push(
        eq === -1
          ? { name: piece, value: "", hasEquals: false }
          : { name: piece.slice(0, eq), value: piece.slice(eq + 1), hasEquals: true },
      );
    }
    if (amp === -1) break;
    start = amp + 1;
  }
  return pairs;
}

const isBlank = (pair: QueryPair): boolean => pair.name === "" && pair.value === "" && pair.hasEquals !== true;

/** Joins pairs with `&`; `name` alone when the value is empty and the pair had no `=`. No encoding. */
export function joinQuery(pairs: readonly QueryPair[]): string {
  return pairs
    .filter((pair) => !isBlank(pair))
    .map((pair) => (pair.value === "" && pair.hasEquals !== true ? pair.name : `${pair.name}=${pair.value}`))
    .join("&");
}

const toRow = (pair: QueryPair, enabled: boolean): ParamRow => ({
  name: pair.name,
  value: pair.value,
  hasEquals: pair.hasEquals,
  enabled,
});

function enabledRowsFromUrl(url: string): readonly ParamRow[] {
  const { query } = splitUrl(url);
  return query === null ? [] : parseQuery(query).map((pair) => toRow(pair, true));
}

/** Enabled rows from the URL, then disabled rows (position is not stored). */
export function rowsFromContent(
  url: string,
  disabledParams: readonly QueryPair[],
): readonly ParamRow[] {
  return [...enabledRowsFromUrl(url), ...disabledParams.map((pair) => toRow(pair, false))];
}

/** Re-derives enabled rows from the edited URL; disabled rows keep their index (clamped). */
export function applyUrlEdit(rows: readonly ParamRow[], newUrl: string): readonly ParamRow[] {
  const merged = [...enabledRowsFromUrl(newUrl)];
  rows.forEach((row, index) => {
    if (row.enabled) return;
    merged.splice(Math.min(index, merged.length), 0, row);
  });
  return merged;
}

/** Rewrites only the query of `url`; disabled rows go to `disabled_params`. */
export function applyRowsEdit(url: string, rows: readonly ParamRow[]): ParamsEdit {
  const { base, fragment } = splitUrl(url);
  const query = joinQuery(rows.filter((row) => row.enabled));
  const disabled = rows
    .filter((row) => !row.enabled && !isBlank(row))
    .map((row) => ({ name: row.name, value: row.value }));
  return {
    url: `${base}${query === "" ? "" : `?${query}`}${fragment}`,
    disabled_params: disabled,
  };
}
