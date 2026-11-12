export const MAX_MATCHES = 1000;

export interface Match {
  readonly start: number;
  readonly end: number;
}

export interface SearchResult {
  readonly matches: readonly Match[];
  /** True when more than `MAX_MATCHES` exist and the list was cut. */
  readonly capped: boolean;
}

export interface Segment {
  readonly text: string;
  /** Index into the match list, or `null` for plain text. */
  readonly match: number | null;
}

const escapeRegex = (s: string): string => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** Case-insensitive, non-overlapping literal search. */
export function findMatches(text: string, query: string): SearchResult {
  if (query === "") return { matches: [], capped: false };
  const re = new RegExp(escapeRegex(query), "gi");
  const matches: Match[] = [];
  for (const m of text.matchAll(re)) {
    if (matches.length === MAX_MATCHES) return { matches, capped: true };
    matches.push({ start: m.index, end: m.index + m[0].length });
  }
  return { matches, capped: false };
}

export function toSegments(text: string, matches: readonly Match[]): readonly Segment[] {
  const segments: Segment[] = [];
  let cursor = 0;
  matches.forEach((m, index) => {
    if (m.start > cursor) segments.push({ text: text.slice(cursor, m.start), match: null });
    segments.push({ text: text.slice(m.start, m.end), match: index });
    cursor = m.end;
  });
  if (cursor < text.length) segments.push({ text: text.slice(cursor), match: null });
  return segments;
}

/** Moves `current` by `delta`, wrapping around `count`. */
export function stepIndex(current: number, count: number, delta: 1 | -1): number {
  if (count === 0) return 0;
  return (current + delta + count) % count;
}
