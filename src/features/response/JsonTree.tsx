import { useMemo, useState, type MouseEvent } from "react";
import "./response.css";

const OPEN_DEPTH = 2;
const PAGE_SIZE = 200;

export type JsonPath = readonly (string | number)[];

export interface JsonTreeProps {
  readonly text: string;
  /** When set, right-clicking a node offers "Save to variable…" for its path. */
  readonly onSaveToVariable?: (path: JsonPath) => void;
}

type Entry = readonly [key: string | number, value: unknown];

const isContainer = (value: unknown): value is object => typeof value === "object" && value !== null;

const entriesOf = (value: object): readonly Entry[] =>
  Array.isArray(value) ? value.map((v, i) => [i, v] as const) : Object.entries(value);

const summary = (value: object): string =>
  Array.isArray(value) ? `[${value.length}]` : `{${Object.keys(value).length}}`;

function Primitive({ value }: { readonly value: unknown }) {
  const kind = value === null ? "null" : typeof value;
  return <span className={`json-${kind}`}>{kind === "string" ? JSON.stringify(value) : String(value)}</span>;
}

interface NodeProps {
  readonly name: string | number;
  readonly value: unknown;
  readonly depth: number;
  readonly path: JsonPath;
  readonly onSave?: (path: JsonPath) => void;
}

function Node({ name, value, depth, path, onSave }: NodeProps) {
  const [open, setOpen] = useState(depth < OPEN_DEPTH);
  const [shown, setShown] = useState(PAGE_SIZE);

  const rowProps = {
    className: "json-row",
    onContextMenu: onSave
      ? (event: MouseEvent<HTMLLIElement>) => {
          event.preventDefault();
          event.stopPropagation();
          onSave(path);
        }
      : undefined,
  };
  if (!isContainer(value)) {
    return (
      <li {...rowProps}>
        <span className="json-key">{name}</span>: <Primitive value={value} />
      </li>
    );
  }
  const entries = entriesOf(value);
  return (
    <li {...rowProps}>
      <button
        type="button"
        className="json-toggle"
        aria-expanded={open}
        aria-label={`${String(name)} ${summary(value)}`}
        onClick={() => setOpen(!open)}
      >
        {open ? "-" : "+"}
      </button>{" "}
      <span className="json-key">{name}</span> <span className="json-summary">{summary(value)}</span>
      {open ? (
        <ul className="json-children">
          {entries.slice(0, shown).map(([key, child]) => (
            <Node key={key} name={key} value={child} depth={depth + 1} path={[...path, key]} onSave={onSave} />
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
export function JsonTree({ text, onSaveToVariable }: JsonTreeProps) {
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
      <Node name="root" value={parsed.value} depth={0} path={[]} onSave={onSaveToVariable} />
    </ul>
  );
}
