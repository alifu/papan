import { useMemo, useState } from "react";
import "./response.css";

const OPEN_DEPTH = 2;
const PAGE_SIZE = 200;

export interface JsonTreeProps {
  readonly text: string;
}

type Entry = readonly [key: string, value: unknown];

const isContainer = (value: unknown): value is object => typeof value === "object" && value !== null;

const entriesOf = (value: object): readonly Entry[] =>
  Array.isArray(value) ? value.map((v, i) => [String(i), v] as const) : Object.entries(value);

const summary = (value: object): string =>
  Array.isArray(value) ? `[${value.length}]` : `{${Object.keys(value).length}}`;

function Primitive({ value }: { readonly value: unknown }) {
  const kind = value === null ? "null" : typeof value;
  return <span className={`json-${kind}`}>{kind === "string" ? JSON.stringify(value) : String(value)}</span>;
}

function Node({ name, value, depth }: { readonly name: string; readonly value: unknown; readonly depth: number }) {
  const [open, setOpen] = useState(depth < OPEN_DEPTH);
  const [shown, setShown] = useState(PAGE_SIZE);

  if (!isContainer(value)) {
    return (
      <li className="json-row">
        <span className="json-key">{name}</span>: <Primitive value={value} />
      </li>
    );
  }
  const entries = entriesOf(value);
  return (
    <li className="json-row">
      <button
        type="button"
        className="json-toggle"
        aria-expanded={open}
        aria-label={`${name} ${summary(value)}`}
        onClick={() => setOpen(!open)}
      >
        {open ? "-" : "+"}
      </button>{" "}
      <span className="json-key">{name}</span> <span className="json-summary">{summary(value)}</span>
      {open ? (
        <ul className="json-children">
          {entries.slice(0, shown).map(([key, child]) => (
            <Node key={key} name={key} value={child} depth={depth + 1} />
          ))}
          {entries.length > shown ? (
            <li>
              <button type="button" className="btn" onClick={() => setShown(shown + PAGE_SIZE)}>
                Show more ({entries.length - shown} left)
              </button>
            </li>
          ) : null}
        </ul>
      ) : null}
    </li>
  );
}

/** Collapsible tree; the first two levels are open, long lists are paged. */
export function JsonTree({ text }: JsonTreeProps) {
  const parsed = useMemo<{ readonly ok: true; readonly value: unknown } | { readonly ok: false }>(() => {
    try {
      return { ok: true, value: JSON.parse(text) as unknown };
    } catch {
      return { ok: false };
    }
  }, [text]);

  if (!parsed.ok) return <p className="json-error">Not valid JSON.</p>;
  return (
    <ul className="json-tree">
      <Node name="root" value={parsed.value} depth={0} />
    </ul>
  );
}
